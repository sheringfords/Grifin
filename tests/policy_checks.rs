//! Hand-computable policy validation + cross-run determinism.
//!
//! Each test here encodes a trace whose correct outcome can be computed by
//! hand from the paper/definition. If the simulator disagrees, the
//! simulator (or the policy transcription) is wrong — not the test.

use grifin::policies;
use grifin::policy::PolicyCtx;
use grifin::simulator::{replay, SimSpec, StorageSpec, TierSpec};
use grifin::trace::{Op, PageId, PageType, TraceEvent};

fn ev(seq: u64, page: PageId, op: Op) -> TraceEvent {
    TraceEvent {
        seq,
        page_id: page,
        relation_id: 0,
        op,
        page_type: PageType::Heap,
        txn_id: seq / 8,
        size_bytes: 8192,
        relation_kind: None,
        logical_group: None,
        creation_time: None,
    }
}

fn trace(pages: &[PageId]) -> Vec<TraceEvent> {
    pages
        .iter()
        .enumerate()
        .map(|(i, &p)| ev(i as u64, p, Op::Read))
        .collect()
}

/// Single-tier mode: everything in T0 (cap `c`), T1 disabled.
fn single_tier(c: usize) -> SimSpec {
    SimSpec {
        t0: TierSpec {
            cap_pages: c,
            read_ns: 1,
            write_ns: 1,
        },
        t1: TierSpec {
            cap_pages: 0,
            read_ns: 1,
            write_ns: 1,
        },
        storage: StorageSpec {
            read_ns: 100,
            write_ns: 100,
        },
        mig_ns: 10,
        page_bytes: 8192,
        warmup_frac: 0.0,
    }
}

fn run(policy: &str, pages: &[PageId], spec: &SimSpec) -> grifin::metrics::Metrics {
    let t = trace(pages);
    let ctx = PolicyCtx {
        t0_cap: spec.t0.cap_pages,
        t1_cap: spec.t1.cap_pages,
    };
    let mut p = policies::make(policy, ctx).unwrap();
    replay(&t, &mut *p, spec).metrics
}

#[test]
fn lru_evicts_cold_tail() {
    // cap 3: A B C A B D -> D evicts C (LRU tail); final A,B hits.
    let m = run("lru", &[1, 2, 3, 1, 2, 4, 1, 2], &single_tier(3));
    assert_eq!(m.misses, 4, "A,B,C,D miss");
    assert_eq!(m.hits(), 4, "A,B then A,B hit");
}

#[test]
fn clock_hand_clears_refbits() {
    // cap 2: A B A C -> C evicts A (both refbits set, hand sweeps once).
    // Then A misses again.
    let m = run("clock", &[1, 2, 1, 3, 1], &single_tier(2));
    assert_eq!(m.misses, 4);
    assert_eq!(m.hits(), 1);
}

#[test]
fn arc_is_scan_resistant_where_lru_is_not() {
    // cap 3 single-tier. A,B hot (two touches => T2); scan C,D,E once;
    // then A must still hit under ARC but miss under LRU.
    let pages = [1, 2, 1, 2, 3, 4, 5, 1];
    let m_arc = run("arc", &pages, &single_tier(3));
    let m_lru = run("lru", &pages, &single_tier(3));
    assert_eq!(m_arc.misses, 5, "ARC: A,B,C,D,E miss; A,B,A,B hit");
    assert_eq!(m_arc.hits(), 3);
    assert_eq!(m_lru.misses, 6, "LRU loses A to the scan");
    assert_eq!(m_lru.hits(), 2);
}

#[test]
fn tinylfu_rejects_one_hit_wonders() {
    // cap 2 single-tier. A,B hot (graduated to protected); C once.
    // C must be rejected (est 1 < est incumbent 2); A,B keep hitting.
    let m = run("tinylfu", &[1, 2, 1, 2, 3, 1, 2], &single_tier(2));
    assert_eq!(m.rejections, 1, "one-hit-wonder C rejected");
    assert_eq!(m.misses, 3, "A,B,C miss");
    assert_eq!(m.hits(), 4);
}

#[test]
fn lirs_bootstraps_and_protects() {
    // Two-tier T0=2/T1=2 (L=2). A,B,A,B: both become LIR in T0; C enters
    // as HIR in T1 and must NOT dislodge them; A,B keep hitting.
    let spec = SimSpec {
        t0: TierSpec {
            cap_pages: 2,
            read_ns: 1,
            write_ns: 1,
        },
        t1: TierSpec {
            cap_pages: 2,
            read_ns: 1,
            write_ns: 1,
        },
        storage: StorageSpec {
            read_ns: 100,
            write_ns: 100,
        },
        mig_ns: 10,
        page_bytes: 8192,
        warmup_frac: 0.0,
    };
    let m = run("lirs", &[1, 2, 1, 2, 3, 1, 2], &spec);
    assert_eq!(m.hits(), 4, "A,B hits + A,B hits after C");
    assert_eq!(m.misses, 3);
}

#[test]
fn static_admits_only_its_slice() {
    // static admits page_id % 4 < 1: pages 4,8 admitted; 1,2,3,5 rejected.
    let m = run("static", &[4, 1, 8, 2, 4, 8], &single_tier(4));
    assert_eq!(m.admissions_t0 + m.admissions_t1, 2);
    assert_eq!(m.rejections, 2);
    assert_eq!(m.hits(), 2);
}

#[test]
fn grifin_bypass_and_promote_paths() {
    // Tiny two-tier: unseen pages start in T1 (value < TH_T0 on 1st touch),
    // repeated pages promote. Just assert internal consistency + no panic;
    // exact values are covered by determinism tests below.
    let spec = SimSpec {
        t1: TierSpec {
            cap_pages: 4,
            read_ns: 1,
            write_ns: 1,
        },
        ..single_tier(2)
    };
    for g in [
        "grifin-reuse",
        "grifin-reuse-write",
        "grifin-reuse-relation",
        "grifin-full",
    ] {
        let m = run(g, &[1, 2, 1, 2, 3, 1, 2, 3, 3, 3], &spec);
        m.reconcile().unwrap();
    }
}

#[test]
fn determinism_same_seed_same_result() {
    use grifin::experiment::run_single;
    use grifin::workload::WorkloadSpec;
    let raw = std::fs::read_to_string("workloads/w3_hotset_shift.json").unwrap();
    let spec_full: WorkloadSpec = serde_json::from_str(&raw).unwrap();
    // Scaled-down trace: exercises all phases/policies quickly. Full-size
    // determinism is covered by results/ manifests (identical seeds rerun).
    let spec = spec_full.scaled(4000);
    let sim = SimSpec::default_hierarchy();
    for p in policies::all_names() {
        let a = run_single(&spec, p, 7, &sim);
        let b = run_single(&spec, p, 7, &sim);
        // Wall-clock fields are excluded by construction: policy CPU timing
        // is measured, not simulated, and can never be bit-identical.
        // Everything else must match exactly.
        let det = |r: &grifin::metrics::RunRecord| {
            format!(
                "{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                (r.mean_lat_ns, r.p50_lat_ns, r.p95_lat_ns, r.p99_lat_ns),
                r.hit_ratio,
                r.physical_reads,
                r.physical_writes,
                r.bytes_written,
                r.migrations,
                r.promotions,
                r.demotions,
                r.evictions,
                r.admissions,
                r.rejections,
                r.writebacks,
                r.j_score,
                r.metadata_bytes,
                r.events,
                r.policy_config,
            )
        };
        assert_eq!(det(&a), det(&b), "nondeterminism in {p}");
    }
}
