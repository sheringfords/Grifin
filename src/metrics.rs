//! Metrics engine: counters, latency distributions, and the total-cost objective.
//!
//! Raw metrics are always reported alongside any composite score (see
//! `docs/EXPERIMENT.md`). Composite coefficients are documented in
//! [`COST_COEFFS`] and frozen before final benchmarking; [`sensitivity_j`]
//! recomputes rankings under perturbed coefficients.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Coefficients of the composite total-cost objective J (per-access form):
///
/// ```text
/// J = mean_lat_ns
///   + WRITE_W * (physical_writes / accesses)
///   + MIGR_W  * (migrations / accesses)
///   + CPU_W   * policy_ns_per_access
/// ```
///
/// Rationale (documented, not tuned): one 4KiB NVMe write costs on the order
/// of 10us end-to-end once controller/GC overhead is included; we use a
/// conservative 4000ns proxy weight *on top of* the already-modelled media
/// latency to represent endurance/write-amplification pressure. A migration
/// is an extra tier read+write (~100ns of modelled latency); the 200ns
/// weight penalises churn beyond raw latency. Policy CPU is added at face
/// value (1ns simulator CPU ~= 1ns of modelled cost).
///
/// These are explicitly simulation parameters. `sensitivity_j` varies them
/// 4x in each direction to check that conclusions do not hinge on them.
#[derive(Clone, Copy, Debug)]
pub struct CostCoeffs {
    pub write_w: f64,
    pub migr_w: f64,
    pub cpu_w: f64,
}

pub const COST_COEFFS: CostCoeffs = CostCoeffs {
    write_w: 4000.0,
    migr_w: 200.0,
    cpu_w: 1.0,
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    pub accesses: u64,
    pub warmup_skipped: u64,
    // hits / misses
    pub hits_t0: u64,
    pub hits_t1: u64,
    pub misses: u64,
    // logical vs physical I/O
    pub logical_reads: u64,
    pub logical_writes: u64,
    pub physical_reads: u64,
    pub physical_writes: u64,
    pub bytes_written: u64,
    // movement
    pub admissions_t0: u64,
    pub admissions_t1: u64,
    pub rejections: u64,
    pub evictions_t0: u64,
    pub evictions_t1: u64,
    pub promotions: u64,
    pub demotions: u64,
    pub writebacks: u64,
    // modelled latency (nanoseconds, simulated)
    pub total_lat_ns: u128,
    /// Per-access modelled latency in ns (post-warmup only). Used for
    /// p50/p95/p99. Traces are small enough that storing this is fine.
    #[serde(skip)]
    pub latencies: Vec<u64>,
}

impl Metrics {
    pub fn hits(&self) -> u64 {
        self.hits_t0 + self.hits_t1
    }

    pub fn hit_ratio(&self) -> f64 {
        if self.accesses == 0 {
            0.0
        } else {
            self.hits() as f64 / self.accesses as f64
        }
    }

    pub fn mean_lat_ns(&self) -> f64 {
        if self.accesses == 0 {
            0.0
        } else {
            self.total_lat_ns as f64 / self.accesses as f64
        }
    }

    fn percentile(&self, q: f64) -> f64 {
        if self.latencies.is_empty() {
            return 0.0;
        }
        let mut v = self.latencies.clone();
        v.sort_unstable();
        let idx = ((q * v.len() as f64).ceil() as usize).saturating_sub(1);
        v[idx.min(v.len() - 1)] as f64
    }

    pub fn p50_lat_ns(&self) -> f64 {
        self.percentile(0.50)
    }
    pub fn p95_lat_ns(&self) -> f64 {
        self.percentile(0.95)
    }
    pub fn p99_lat_ns(&self) -> f64 {
        self.percentile(0.99)
    }

    pub fn phys_writes_per_access(&self) -> f64 {
        self.physical_writes as f64 / self.accesses.max(1) as f64
    }

    pub fn migrations_per_access(&self) -> f64 {
        (self.promotions + self.demotions) as f64 / self.accesses.max(1) as f64
    }

    /// Primary composite objective J with explicit coefficients.
    /// Policy CPU is EXCLUDED by design: J measures mechanism quality
    /// (modelled latency + endurance + churn), while measured wall-clock
    /// policy overhead is reported separately and gated by the
    /// "overhead << benefit" check. Rationale: J must not hinge on how
    /// aggressively each policy implementation was micro-optimised; see
    /// `docs/EXPERIMENT.md`. `j_total` adds CPU back for system-level view.
    pub fn j(&self, c: CostCoeffs) -> f64 {
        self.mean_lat_ns()
            + c.write_w * self.phys_writes_per_access()
            + c.migr_w * self.migrations_per_access()
    }

    pub fn j_total(&self, c: CostCoeffs, policy_ns_per_access: f64) -> f64 {
        self.j(c) + c.cpu_w * policy_ns_per_access
    }

    /// Reconcile counters: fail loudly on inconsistency.
    pub fn reconcile(&self) -> Result<(), String> {
        if self.hits() + self.misses != self.accesses {
            return Err(format!(
                "hits({}) + misses({}) != accesses({})",
                self.hits(),
                self.misses,
                self.accesses
            ));
        }
        if self.logical_reads + self.logical_writes != self.accesses {
            return Err(format!(
                "logical reads({}) + writes({}) != accesses({})",
                self.logical_reads, self.logical_writes, self.accesses
            ));
        }
        if self.admissions_t0 + self.admissions_t1 + self.rejections != self.misses {
            return Err(format!(
                "admissions({}+{}) + rejections({}) != misses({})",
                self.admissions_t0, self.admissions_t1, self.rejections, self.misses
            ));
        }
        if self.latencies.len() as u64 != self.accesses {
            return Err(format!(
                "latency samples({}) != accesses({})",
                self.latencies.len(),
                self.accesses
            ));
        }
        Ok(())
    }
}

/// Aggregate statistics across seeds for one (workload, policy) cell.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellStats {
    pub n: usize,
    pub median: f64,
    pub mean: f64,
    pub std: f64,
    pub ci95_halfwidth: f64,
    pub min: f64,
    pub max: f64,
}

impl CellStats {
    pub fn of(mut xs: Vec<f64>) -> Self {
        assert!(!xs.is_empty(), "CellStats::of with no samples");
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = xs.len();
        let median = if n % 2 == 1 {
            xs[n / 2]
        } else {
            (xs[n / 2 - 1] + xs[n / 2]) / 2.0
        };
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = if n > 1 {
            xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        let std = var.sqrt();
        // Normal-approx 95% CI half-width. With n=5 this is optimistic;
        // we report min/max alongside so readers can see the spread.
        let ci95_halfwidth = if n > 1 {
            1.96 * std / (n as f64).sqrt()
        } else {
            0.0
        };
        Self {
            n,
            median,
            mean,
            std,
            ci95_halfwidth,
            min: xs[0],
            max: xs[n - 1],
        }
    }
}

/// Flat per-run record written to `metrics.json` (machine-readable).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub workload: String,
    pub policy: String,
    pub policy_config: String,
    pub seed: u64,
    pub events: usize,
    pub warmup_skipped: u64,
    pub mean_lat_ns: f64,
    pub p50_lat_ns: f64,
    pub p95_lat_ns: f64,
    pub p99_lat_ns: f64,
    pub hit_ratio: f64,
    pub hit_ratio_t0: f64,
    pub hit_ratio_t1: f64,
    pub physical_reads: u64,
    pub physical_writes: u64,
    pub bytes_written: u64,
    pub migrations: u64,
    pub promotions: u64,
    pub demotions: u64,
    pub evictions: u64,
    pub admissions: u64,
    pub rejections: u64,
    pub writebacks: u64,
    pub j_score: f64,
    pub policy_ns_per_access: f64,
    pub metadata_bytes: u64,
    /// outcomes may be big; keep the map small and explicit.
    pub extra: HashMap<String, f64>,
}

impl RunRecord {
    pub fn metric(&self, name: &str) -> Option<f64> {
        match name {
            "mean_lat_ns" => Some(self.mean_lat_ns),
            "p99_lat_ns" => Some(self.p99_lat_ns),
            "j_score" => Some(self.j_score),
            "hit_ratio" => Some(self.hit_ratio),
            "physical_writes" => Some(self.physical_writes as f64),
            _ => self.extra.get(name).copied(),
        }
    }
}
