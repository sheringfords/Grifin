//! Experiment runner: full policy x workload x seed matrix with provenance.
//!
//! Layout:
//! ```text
//! results/<exp-id>/
//!   manifest.json    provenance (git SHA, toolchain, OS/CPU, configs)
//!   config.json      tier + cost-coefficient configuration
//!   runs/<workload>_<policy>_seed<seed>.json
//!   summary.json     per-cell aggregates across seeds
//!   SUMMARY.md       human-readable tables (generated, not hand-copied)
//! ```

use crate::metrics::{CellStats, RunRecord, COST_COEFFS};
use crate::policies;
use crate::policy::PolicyCtx;
use crate::simulator::{replay, SimSpec};
use crate::workload::{generate, load_spec, WorkloadSpec};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub experiment_id: String,
    pub git_sha: String,
    pub git_dirty: bool,
    pub rustc_version: String,
    pub os: String,
    pub arch: String,
    pub cpu_count: usize,
    pub timestamp_utc: String,
    pub workload_specs: HashMap<String, String>, // id -> fnv hash of spec file
    pub tier_config: String,
    pub cost_coeffs: CostCoeffsSer,
    pub seeds: Vec<u64>,
    pub events_per_workload: u64,
    pub warmup_frac: f64,
    pub policies: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CostCoeffsSer {
    pub write_w: f64,
    pub migr_w: f64,
    pub cpu_w: f64,
}

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string()
}

fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

pub struct MatrixConfig {
    pub out_dir: String,
    pub workload_dir: String,
    pub seeds: Vec<u64>,
    pub events: u64,
    pub sim: SimSpec,
    pub policies: Vec<String>,
    pub experiment_id: String,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            out_dir: "results/dev".into(),
            workload_dir: "workloads".into(),
            seeds: vec![1, 2, 3],
            events: 60_000,
            sim: SimSpec::default_hierarchy(),
            policies: policies::all_names()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            experiment_id: "dev".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellSummary {
    pub workload: String,
    pub policy: String,
    pub policy_config: String,
    pub seeds: Vec<u64>,
    pub mean_lat_ns: CellStats,
    pub p99_lat_ns: CellStats,
    pub j_score: CellStats,
    pub hit_ratio: CellStats,
    pub physical_writes: CellStats,
    pub migrations: CellStats,
    pub policy_ns_per_access: CellStats,
    pub metadata_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub manifest: Manifest,
    pub cells: Vec<CellSummary>,
}

fn policy_ns(m: &crate::metrics::Metrics, wall_ns: u64) -> f64 {
    wall_ns as f64 / m.accesses.max(1) as f64
}

pub fn run_single(spec: &WorkloadSpec, policy_name: &str, seed: u64, sim: &SimSpec) -> RunRecord {
    let trace = generate(spec, seed);
    let ctx = PolicyCtx {
        t0_cap: sim.t0.cap_pages,
        t1_cap: sim.t1.cap_pages,
    };
    let mut policy = policies::make(policy_name, ctx)
        .unwrap_or_else(|e| panic!("cannot build policy {policy_name}: {e}"));
    let config_str = policy.config_str();
    let out = replay(&trace, &mut *policy, sim);
    let m = &out.metrics;
    let pns = policy_ns(m, out.policy_wall_ns);
    let mut extra = HashMap::new();
    for (k, v) in out.extra {
        extra.insert(k, v);
    }
    extra.insert("hit_ratio_t0_raw".into(), m.hits_t0 as f64);
    extra.insert("j_total".into(), m.j_total(COST_COEFFS, pns));
    RunRecord {
        workload: spec.id.clone(),
        policy: policy_name.into(),
        policy_config: config_str,
        seed,
        events: trace.len(),
        warmup_skipped: m.warmup_skipped,
        mean_lat_ns: m.mean_lat_ns(),
        p50_lat_ns: m.p50_lat_ns(),
        p95_lat_ns: m.p95_lat_ns(),
        p99_lat_ns: m.p99_lat_ns(),
        hit_ratio: m.hit_ratio(),
        hit_ratio_t0: m.hits_t0 as f64 / m.accesses.max(1) as f64,
        hit_ratio_t1: m.hits_t1 as f64 / m.accesses.max(1) as f64,
        physical_reads: m.physical_reads,
        physical_writes: m.physical_writes,
        bytes_written: m.bytes_written,
        migrations: m.promotions + m.demotions,
        promotions: m.promotions,
        demotions: m.demotions,
        evictions: m.evictions_t0 + m.evictions_t1,
        admissions: m.admissions_t0 + m.admissions_t1,
        rejections: m.rejections,
        writebacks: m.writebacks,
        j_score: m.j(COST_COEFFS),
        policy_ns_per_access: pns,
        metadata_bytes: out.metadata_bytes,
        extra,
    }
}

pub fn run_matrix(cfg: &MatrixConfig) -> Summary {
    let runs_dir = format!("{}/runs", cfg.out_dir);
    std::fs::create_dir_all(&runs_dir).expect("cannot create results dir");

    // Load + scale specs.
    let mut specs: Vec<(String, WorkloadSpec)> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&cfg.workload_dir)
        .unwrap_or_else(|_| panic!("cannot read {}", cfg.workload_dir))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no workload specs found");
    let mut spec_hashes = HashMap::new();
    for path in &entries {
        let raw = std::fs::read(path).expect("read spec");
        spec_hashes.insert(
            path.file_stem().unwrap().to_string_lossy().to_string(),
            fnv1a(&raw),
        );
        let spec = load_spec(path.to_str().unwrap()).expect("load spec");
        specs.push((spec.id.clone(), spec.scaled(cfg.events)));
    }

    let manifest = Manifest {
        experiment_id: cfg.experiment_id.clone(),
        git_sha: cmd_output("git", &["rev-parse", "HEAD"]),
        git_dirty: !cmd_output("git", &["status", "--porcelain"]).is_empty(),
        rustc_version: cmd_output("rustc", &["--version"]),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        timestamp_utc: cmd_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        workload_specs: spec_hashes,
        tier_config: cfg.sim.describe(),
        cost_coeffs: CostCoeffsSer {
            write_w: COST_COEFFS.write_w,
            migr_w: COST_COEFFS.migr_w,
            cpu_w: COST_COEFFS.cpu_w,
        },
        seeds: cfg.seeds.clone(),
        events_per_workload: cfg.events,
        warmup_frac: cfg.sim.warmup_frac,
        policies: cfg.policies.clone(),
    };
    std::fs::write(
        format!("{}/manifest.json", cfg.out_dir),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .expect("write manifest");
    std::fs::write(
        format!("{}/config.json", cfg.out_dir),
        format!(
            "{{\n  \"tier\": \"{}\",\n  \"events\": {},\n  \"seeds\": {:?}\n}}\n",
            cfg.sim.describe(),
            cfg.events,
            cfg.seeds
        ),
    )
    .expect("write config");

    // Run the matrix.
    let total = specs.len() * cfg.policies.len() * cfg.seeds.len();
    let mut done = 0;
    let t0 = Instant::now();
    let mut records: Vec<RunRecord> = Vec::new();
    for (_, spec) in &specs {
        for policy in &cfg.policies {
            for &seed in &cfg.seeds {
                let rec = run_single(spec, policy, seed, &cfg.sim);
                std::fs::write(
                    format!(
                        "{}/{}_{}_seed{}.json",
                        runs_dir, rec.workload, rec.policy, rec.seed
                    ),
                    serde_json::to_string_pretty(&rec).unwrap(),
                )
                .expect("write run");
                records.push(rec);
                done += 1;
                if done % 20 == 0 || done == total {
                    eprintln!(
                        "  [{done}/{total}] elapsed {:.1}s",
                        t0.elapsed().as_secs_f64()
                    );
                }
            }
        }
    }

    // Aggregate across seeds.
    let mut cells: Vec<CellSummary> = Vec::new();
    for (_, spec) in &specs {
        for policy in &cfg.policies {
            let rs: Vec<&RunRecord> = records
                .iter()
                .filter(|r| r.workload == spec.id && r.policy == *policy)
                .collect();
            assert!(!rs.is_empty());
            let col = |f: fn(&RunRecord) -> f64| -> CellStats {
                CellStats::of(rs.iter().map(|r| f(r)).collect())
            };
            cells.push(CellSummary {
                workload: spec.id.clone(),
                policy: policy.clone(),
                policy_config: rs[0].policy_config.clone(),
                seeds: cfg.seeds.clone(),
                mean_lat_ns: col(|r| r.mean_lat_ns),
                p99_lat_ns: col(|r| r.p99_lat_ns),
                j_score: col(|r| r.j_score),
                hit_ratio: col(|r| r.hit_ratio),
                physical_writes: col(|r| r.physical_writes as f64),
                migrations: col(|r| r.migrations as f64),
                policy_ns_per_access: col(|r| r.policy_ns_per_access),
                metadata_bytes: rs[0].metadata_bytes,
            });
        }
    }

    let summary = Summary { manifest, cells };
    std::fs::write(
        format!("{}/summary.json", cfg.out_dir),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .expect("write summary");
    let md = crate::report::render_summary(&summary);
    std::fs::write(format!("{}/SUMMARY.md", cfg.out_dir), md).expect("write SUMMARY.md");
    summary
}
