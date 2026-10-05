//! Simulator kernel: deterministic replay of a trace against a policy.
//!
//! The simulator owns authoritative state: which pages are resident where,
//! dirty bits, and tier occupancy. Policies are advisors. Every advisory
//! answer is checked against ground truth; violations panic.
//!
//! Cost model (all simulation parameters, see `docs/RESEARCH.md`):
//! - hit: tier read/write latency.
//! - miss + admit: storage read + tier fill write (+ writeback if the victim
//!   it displaces is dirty; + migration if a T0 victim is demoted to T1).
//! - miss + bypass: storage read (+ storage write for WRITE ops).
//! - T1->T0 promotion: migration cost.
//!
//! Zero-capacity tiers are skipped (T1->T0 redirect): this gives a clean
//! single-tier mode used to hand-validate policies.

use crate::metrics::Metrics;
use crate::policy::{Decision, Policy, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct TierSpec {
    pub cap_pages: usize,
    pub read_ns: u64,
    pub write_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct StorageSpec {
    pub read_ns: u64,
    pub write_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct SimSpec {
    pub t0: TierSpec,
    pub t1: TierSpec,
    pub storage: StorageSpec,
    /// Cost of one T0<->T1 migration (modeled tier-to-tier copy).
    pub mig_ns: u64,
    pub page_bytes: u64,
    /// Fraction of leading events treated as warmup (state updates apply,
    /// metrics excluded). Must be in [0, 0.5).
    pub warmup_frac: f64,
}

impl SimSpec {
    /// Default hierarchy: 512 fast pages, 2048 slow pages, 8KiB pages.
    /// Latencies are simulation parameters, not hardware claims.
    pub fn default_hierarchy() -> Self {
        Self {
            t0: TierSpec {
                cap_pages: 512,
                read_ns: 80,
                write_ns: 120,
            },
            t1: TierSpec {
                cap_pages: 2048,
                read_ns: 400,
                write_ns: 600,
            },
            storage: StorageSpec {
                read_ns: 70_000,
                write_ns: 90_000,
            },
            mig_ns: 1_000,
            page_bytes: 8192,
            warmup_frac: 0.10,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..0.5).contains(&self.warmup_frac) {
            return Err(format!("warmup_frac {} not in [0, 0.5)", self.warmup_frac));
        }
        if self.t0.cap_pages == 0 && self.t1.cap_pages == 0 {
            return Err("both tiers have zero capacity".to_string());
        }
        if self.page_bytes == 0 {
            return Err("page_bytes must be > 0".to_string());
        }
        Ok(())
    }

    pub fn describe(&self) -> String {
        format!(
            "t0(cap={},r={},w={}) t1(cap={},r={},w={}) stor(r={},w={}) mig={} page={}B warmup={}",
            self.t0.cap_pages,
            self.t0.read_ns,
            self.t0.write_ns,
            self.t1.cap_pages,
            self.t1.read_ns,
            self.t1.write_ns,
            self.storage.read_ns,
            self.storage.write_ns,
            self.mig_ns,
            self.page_bytes,
            self.warmup_frac,
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    tier: Tier,
    dirty: bool,
}

pub struct Outcome {
    pub metrics: Metrics,
    /// Wall-clock nanoseconds spent inside policy calls (post-warmup).
    pub policy_wall_ns: u64,
    pub metadata_bytes: u64,
    pub extra: HashMap<String, f64>,
}

pub fn replay(trace: &[TraceEvent], policy: &mut dyn Policy, spec: &SimSpec) -> Outcome {
    spec.validate().expect("invalid SimSpec");
    crate::trace::validate_trace(trace).expect("invalid trace");

    let warmup_n = ((trace.len() as f64) * spec.warmup_frac).floor() as usize;
    let mut resident: HashMap<PageId, Slot> = HashMap::new();
    let mut n_t0: usize = 0;
    let mut n_t1: usize = 0;
    let mut m = Metrics::default();
    let mut policy_wall_ns: u64 = 0;
    let mut occ_t0_sum: u64 = 0;
    let mut occ_t1_sum: u64 = 0;

    // Redirect admissions away from zero-capacity tiers (single-tier mode).
    let redirect = |t: Tier| -> Option<Tier> {
        match t {
            Tier::T0 if spec.t0.cap_pages == 0 => {
                if spec.t1.cap_pages == 0 {
                    None
                } else {
                    Some(Tier::T1)
                }
            }
            Tier::T1 if spec.t1.cap_pages == 0 => {
                if spec.t0.cap_pages == 0 {
                    None
                } else {
                    Some(Tier::T0)
                }
            }
            _ => Some(t),
        }
    };

    for (i, ev) in trace.iter().enumerate() {
        let measured = i >= warmup_n;
        let slot = resident.get(&ev.page_id).copied();

        let t_start = Instant::now();
        let decision: Decision = policy.on_access(ev, slot.map(|s| (s.tier, s.dirty)));
        let policy_dt = t_start.elapsed().as_nanos() as u64;

        let mut cost: u64 = 0;
        let mut pns: u64 = policy_dt; // policy wall-clock for this event
        let add = |c: &mut u64, v: u64| {
            *c = c.checked_add(v).expect("simulated latency overflow");
        };

        match slot {
            Some(s) => {
                // ---- HIT ----
                let lat = match (s.tier, ev.op.is_write()) {
                    (Tier::T0, false) => spec.t0.read_ns,
                    (Tier::T0, true) => spec.t0.write_ns,
                    (Tier::T1, false) => spec.t1.read_ns,
                    (Tier::T1, true) => spec.t1.write_ns,
                };
                add(&mut cost, lat);
                let dirty = s.dirty || ev.op.is_write();
                if measured {
                    match s.tier {
                        Tier::T0 => m.hits_t0 += 1,
                        Tier::T1 => m.hits_t1 += 1,
                    }
                    if ev.op.is_write() {
                        m.logical_writes += 1;
                    } else {
                        m.logical_reads += 1;
                    }
                }
                if decision.promote && s.tier == Tier::T1 && spec.t0.cap_pages > 0 {
                    // Promote to T0; make room by demoting a T0 victim into
                    // the freed T1 slot (or evicting it if T1 is full, which
                    // cannot happen here since we just freed a T1 slot...
                    // unless caps are pathological; handle generally).
                    if n_t0 >= spec.t0.cap_pages {
                        let t0v = Instant::now();
                        let v = policy.victim(Tier::T0).unwrap_or_else(|| {
                            panic!(
                                "policy {} returned no T0 victim while T0 full (promote path)",
                                policy.name()
                            )
                        });
                        pns = pns
                            .checked_add(t0v.elapsed().as_nanos() as u64)
                            .expect("policy clock overflow");
                        let vslot = resident.get(&v).copied().unwrap_or_else(|| {
                            panic!(
                                "policy {} victim {v} not resident (promote path)",
                                policy.name()
                            )
                        });
                        assert_eq!(
                            vslot.tier,
                            Tier::T0,
                            "policy {} victim {v} not in T0 (promote path)",
                            policy.name()
                        );
                        resident.remove(&v);
                        n_t0 -= 1;
                        if n_t1 >= spec.t1.cap_pages {
                            // T1 full despite freed slot only if caps are
                            // zero-ish; evict v to storage.
                            let t0e = Instant::now();
                            policy.on_evict(v, Tier::T0);
                            pns = pns
                                .checked_add(t0e.elapsed().as_nanos() as u64)
                                .expect("policy clock overflow");
                            add(&mut cost, spec.mig_ns);
                            if vslot.dirty {
                                add(&mut cost, spec.storage.write_ns);
                                if measured {
                                    m.physical_writes += 1;
                                    m.bytes_written = m
                                        .bytes_written
                                        .checked_add(spec.page_bytes)
                                        .expect("bytes overflow");
                                    m.writebacks += 1;
                                }
                            }
                            if measured {
                                m.evictions_t0 += 1;
                            }
                        } else {
                            let t0m = Instant::now();
                            policy.on_move(v, Tier::T0, Tier::T1);
                            pns = pns
                                .checked_add(t0m.elapsed().as_nanos() as u64)
                                .expect("policy clock overflow");
                            resident.insert(
                                v,
                                Slot {
                                    tier: Tier::T1,
                                    dirty: vslot.dirty,
                                },
                            );
                            n_t1 += 1;
                            add(&mut cost, spec.mig_ns);
                            if measured {
                                m.demotions += 1;
                                m.evictions_t0 += 1;
                            }
                        }
                    }
                    // Move the hit page T1 -> T0 (net T1 count -1 handled below).
                    let tpm = Instant::now();
                    policy.on_move(ev.page_id, Tier::T1, Tier::T0);
                    pns = pns
                        .checked_add(tpm.elapsed().as_nanos() as u64)
                        .expect("policy clock overflow");
                    resident.insert(
                        ev.page_id,
                        Slot {
                            tier: Tier::T0,
                            dirty,
                        },
                    );
                    n_t0 += 1;
                    n_t1 -= 1;
                    add(&mut cost, spec.mig_ns);
                    if measured {
                        m.promotions += 1;
                    }
                } else {
                    resident.insert(
                        ev.page_id,
                        Slot {
                            tier: s.tier,
                            dirty,
                        },
                    );
                }
            }
            None => {
                // ---- MISS ----
                add(&mut cost, spec.storage.read_ns);
                if measured {
                    m.misses += 1;
                    m.physical_reads += 1;
                    if ev.op.is_write() {
                        m.logical_writes += 1;
                    } else {
                        m.logical_reads += 1;
                    }
                }
                let target = decision.admit.and_then(redirect);
                match target {
                    None => {
                        if measured {
                            m.rejections += 1;
                        }
                        if ev.op.is_write() {
                            // Write-through to storage for bypassed writes.
                            add(&mut cost, spec.storage.write_ns);
                            if measured {
                                m.physical_writes += 1;
                                m.bytes_written = m
                                    .bytes_written
                                    .checked_add(spec.page_bytes)
                                    .expect("bytes overflow");
                            }
                        }
                    }
                    Some(tier) => {
                        let (cap, n, fill_lat) = match tier {
                            Tier::T0 => (spec.t0.cap_pages, n_t0, spec.t0.write_ns),
                            Tier::T1 => (spec.t1.cap_pages, n_t1, spec.t1.write_ns),
                        };
                        if n >= cap {
                            let tv = Instant::now();
                            let v = policy.victim(tier).unwrap_or_else(|| {
                                panic!(
                                    "policy {} returned no victim for full {:?}",
                                    policy.name(),
                                    tier
                                )
                            });
                            pns = pns
                                .checked_add(tv.elapsed().as_nanos() as u64)
                                .expect("policy clock overflow");
                            let vslot = resident.get(&v).copied().unwrap_or_else(|| {
                                panic!(
                                    "policy {} victim {v} not resident (admit path)",
                                    policy.name()
                                )
                            });
                            if vslot.tier != tier {
                                panic!(
                                    "policy {} victim {v} in {:?}, expected {:?} (admit path)",
                                    policy.name(),
                                    vslot.tier,
                                    tier
                                );
                            }
                            resident.remove(&v);
                            match tier {
                                Tier::T0 => n_t0 -= 1,
                                Tier::T1 => n_t1 -= 1,
                            }
                            // Demote T0 victims to T1 when there is room;
                            // otherwise (or for T1 victims) evict to storage.
                            let demote_room = tier == Tier::T0 && n_t1 < spec.t1.cap_pages;
                            if demote_room {
                                let tdm = Instant::now();
                                policy.on_move(v, Tier::T0, Tier::T1);
                                pns = pns
                                    .checked_add(tdm.elapsed().as_nanos() as u64)
                                    .expect("policy clock overflow");
                                resident.insert(
                                    v,
                                    Slot {
                                        tier: Tier::T1,
                                        dirty: vslot.dirty,
                                    },
                                );
                                n_t1 += 1;
                                add(&mut cost, spec.mig_ns);
                                if measured {
                                    m.demotions += 1;
                                    m.evictions_t0 += 1;
                                }
                            } else {
                                let te = Instant::now();
                                policy.on_evict(v, tier);
                                pns = pns
                                    .checked_add(te.elapsed().as_nanos() as u64)
                                    .expect("policy clock overflow");
                                if vslot.dirty {
                                    add(&mut cost, spec.storage.write_ns);
                                    if measured {
                                        m.physical_writes += 1;
                                        m.bytes_written = m
                                            .bytes_written
                                            .checked_add(spec.page_bytes)
                                            .expect("bytes overflow");
                                        m.writebacks += 1;
                                    }
                                }
                                if measured {
                                    match tier {
                                        Tier::T0 => m.evictions_t0 += 1,
                                        Tier::T1 => m.evictions_t1 += 1,
                                    }
                                }
                            }
                        }
                        let tpl = Instant::now();
                        policy.on_place(ev.page_id, tier);
                        pns = pns
                            .checked_add(tpl.elapsed().as_nanos() as u64)
                            .expect("policy clock overflow");
                        resident.insert(
                            ev.page_id,
                            Slot {
                                tier,
                                dirty: ev.op.is_write(),
                            },
                        );
                        match tier {
                            Tier::T0 => {
                                n_t0 += 1;
                                if measured {
                                    m.admissions_t0 += 1;
                                }
                            }
                            Tier::T1 => {
                                n_t1 += 1;
                                if measured {
                                    m.admissions_t1 += 1;
                                }
                            }
                        }
                        add(&mut cost, fill_lat);
                    }
                }
            }
        }

        // Capacity invariants (checked every event; cheap).
        assert!(
            n_t0 <= spec.t0.cap_pages,
            "T0 over capacity: {n_t0} > {}",
            spec.t0.cap_pages
        );
        assert!(
            n_t1 <= spec.t1.cap_pages,
            "T1 over capacity: {n_t1} > {}",
            spec.t1.cap_pages
        );
        // Policy membership mirror must match simulator ground truth.
        assert_eq!(
            policy.tracked(),
            resident.len(),
            "policy {} membership drift: tracked={} resident={} at seq {}",
            policy.name(),
            policy.tracked(),
            resident.len(),
            ev.seq
        );

        if measured {
            m.accesses += 1;
            m.total_lat_ns += cost as u128;
            m.latencies.push(cost);
            policy_wall_ns = policy_wall_ns
                .checked_add(pns)
                .expect("policy clock overflow");
            occ_t0_sum += n_t0 as u64;
            occ_t1_sum += n_t1 as u64;
        } else {
            m.warmup_skipped += 1;
        }
    }

    m.reconcile().expect("metrics failed to reconcile");

    // Residency uniqueness holds by construction (single map). Verify tier
    // counts against the map in all builds, not just debug.
    {
        let mut c0 = 0usize;
        let mut c1 = 0usize;
        let mut seen = HashSet::new();
        for (p, s) in resident.iter() {
            assert!(seen.insert(p), "page resident twice: {p}");
            match s.tier {
                Tier::T0 => c0 += 1,
                Tier::T1 => c1 += 1,
            }
        }
        assert_eq!(c0, n_t0, "T0 count drift");
        assert_eq!(c1, n_t1, "T1 count drift");
    }

    let mut extra = HashMap::new();
    if m.accesses > 0 {
        extra.insert(
            "avg_occupancy_t0".to_string(),
            occ_t0_sum as f64 / m.accesses as f64,
        );
        extra.insert(
            "avg_occupancy_t1".to_string(),
            occ_t1_sum as f64 / m.accesses as f64,
        );
    }

    Outcome {
        metrics: m,
        policy_wall_ns,
        metadata_bytes: policy.metadata_bytes(),
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::lru::Lru;
    use crate::trace::{Op, PageType, TraceEvent};

    fn ev(seq: u64, page: u64) -> TraceEvent {
        TraceEvent {
            seq,
            page_id: page,
            relation_id: 0,
            op: Op::Read,
            page_type: PageType::Heap,
            txn_id: 0,
            size_bytes: 8192,
            relation_kind: None,
            logical_group: None,
            creation_time: None,
        }
    }

    fn tiny_spec() -> SimSpec {
        SimSpec {
            t0: TierSpec {
                cap_pages: 2,
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

    #[test]
    fn lru_hand_trace_single_tier() {
        // cap=2, single tier (T1=0 redirects to T0). A B C A B D:
        // A miss, B miss, C miss(evict A), A miss(evict B), B miss(evict C),
        // D miss(evict A). => 6 misses, 0 hits.
        let pages = [1, 2, 3, 1, 2, 4];
        let trace: Vec<_> = pages
            .iter()
            .enumerate()
            .map(|(i, &p)| ev(i as u64, p))
            .collect();
        let mut p = Lru::new(PolicyCtxForTest::ctx());
        let out = replay(&trace, &mut p, &tiny_spec());
        assert_eq!(out.metrics.misses, 6);
        assert_eq!(out.metrics.hits(), 0);
    }

    #[test]
    fn lru_hand_trace_with_hits() {
        // A B A B with cap 2: 2 misses then 2 hits.
        let pages = [1, 2, 1, 2];
        let trace: Vec<_> = pages
            .iter()
            .enumerate()
            .map(|(i, &p)| ev(i as u64, p))
            .collect();
        let mut p = Lru::new(PolicyCtxForTest::ctx());
        let out = replay(&trace, &mut p, &tiny_spec());
        assert_eq!(out.metrics.misses, 2);
        assert_eq!(out.metrics.hits(), 2);
    }

    #[test]
    fn dirty_eviction_writes_back() {
        // Write A (dirty), then B, C with cap 2 single-tier: evicting A must
        // produce exactly one physical write + bytes.
        let mut trace: Vec<TraceEvent> = vec![];
        let mk = |seq: u64, page: u64, op: Op| {
            let mut e = ev(seq, page);
            e.op = op;
            e
        };
        trace.push(mk(0, 1, Op::Write));
        trace.push(mk(1, 2, Op::Read));
        trace.push(mk(2, 3, Op::Read));
        let mut p = Lru::new(PolicyCtxForTest::ctx());
        let out = replay(&trace, &mut p, &tiny_spec());
        assert_eq!(out.metrics.physical_writes, 1);
        assert_eq!(out.metrics.bytes_written, 8192);
        assert_eq!(out.metrics.writebacks, 1);
    }

    // Helper to build a PolicyCtx without importing policies::factory here.
    struct PolicyCtxForTest;
    impl PolicyCtxForTest {
        fn ctx() -> crate::policy::PolicyCtx {
            crate::policy::PolicyCtx {
                t0_cap: 2,
                t1_cap: 0,
            }
        }
    }
}
