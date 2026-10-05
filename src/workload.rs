//! Deterministic synthetic workload generator (phase-based).
//!
//! Workload specifications live in `workloads/*.json` and are parsed here —
//! separately from execution code. Given the same spec + seed, generation is
//! bit-identical across platforms (SplitMix64 + precomputed Zipf tables).
//!
//! Page-type honesty: a relation's first pages are `Inner` (B-tree
//! analogue), the bulk is `Leaf`/`Heap`, blob relations are `Blob`. Zipf
//! rank 0 maps to the lowest offset, so inner pages are genuinely hotter
//! because traversals start there — not by assertion.

use crate::rng::{Rng, Zipf};
use crate::trace::{Op, PageType, TraceEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelationSpec {
    pub id: u32,
    pub kind: String,
    pub base: u64,
    pub len: u64,
    /// First `inner_pages` offsets are Inner type.
    pub inner_pages: u64,
    /// Size of a logical group (for `logical_group` metadata).
    pub group_size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamSpec {
    pub start: u64,
    pub npages: u64,
    pub theta: f64,
    pub read_frac: f64,
    pub weight: f64,
    pub relation: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Phase {
    /// Stable Zipf hotspot.
    Zipf {
        len: u64,
        start: u64,
        npages: u64,
        theta: f64,
        read_frac: f64,
        relation: u32,
    },
    /// Sequential pass over a cold region.
    Scan {
        start: u64,
        npages: u64,
        stride: u64,
        repeats: u64,
        read_frac: f64,
        relation: u32,
    },
    /// Zipf base with a concentrated write burst in the middle.
    WriteBurst {
        len: u64,
        start: u64,
        npages: u64,
        theta: f64,
        base_read_frac: f64,
        burst_write_pages: u64,
        burst_len_frac: f64,
        relation: u32,
    },
    /// Competing Zipf streams (mixed relations).
    Mixed { len: u64, streams: Vec<StreamSpec> },
    /// Sliding hot window: pages are hot for ~`life` events then die.
    Churn {
        len: u64,
        pool_start: u64,
        pool_size: u64,
        window: u64,
        theta: f64,
        read_frac: f64,
        relation: u32,
    },
    /// OLTP base with periodic analytical scans interleaved.
    OltpScan {
        len: u64,
        start: u64,
        npages: u64,
        theta: f64,
        read_frac: f64,
        relation: u32,
        scan_every: u64,
        scan_npages: u64,
        scan_start: u64,
        scan_relation: u32,
    },
}

impl Phase {
    pub fn len_events(&self) -> u64 {
        match *self {
            Phase::Zipf { len, .. }
            | Phase::WriteBurst { len, .. }
            | Phase::Mixed { len, .. }
            | Phase::Churn { len, .. }
            | Phase::OltpScan { len, .. } => len,
            Phase::Scan {
                npages,
                stride,
                repeats,
                ..
            } => (npages / stride.max(1)).saturating_mul(repeats).max(1),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadSpec {
    pub id: String,
    pub description: String,
    pub universe_pages: u64,
    pub page_bytes: u32,
    pub relations: Vec<RelationSpec>,
    pub phases: Vec<Phase>,
}

impl WorkloadSpec {
    pub fn total_events(&self) -> u64 {
        self.phases.iter().map(|p| p.len_events()).sum()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.phases.is_empty() {
            return Err(format!("workload {}: no phases", self.id));
        }
        if self.universe_pages == 0 {
            return Err(format!("workload {}: universe_pages == 0", self.id));
        }
        for r in &self.relations {
            if r.base.checked_add(r.len).is_none() {
                return Err(format!("relation {}: base+len overflow", r.id));
            }
            if r.base + r.len > self.universe_pages {
                return Err(format!(
                    "relation {} exceeds universe ({} > {})",
                    r.id,
                    r.base + r.len,
                    self.universe_pages
                ));
            }
            if r.group_size == 0 {
                return Err(format!("relation {}: group_size == 0", r.id));
            }
        }
        let rel_ids: Vec<u32> = self.relations.iter().map(|r| r.id).collect();
        let known = |id: u32| rel_ids.contains(&id);
        for (i, p) in self.phases.iter().enumerate() {
            match p {
                Phase::Zipf { relation, .. }
                | Phase::Scan { relation, .. }
                | Phase::WriteBurst { relation, .. }
                | Phase::Churn { relation, .. }
                | Phase::OltpScan { relation, .. } => {
                    if !known(*relation) {
                        return Err(format!("phase {i}: unknown relation {relation}"));
                    }
                }
                Phase::Mixed { streams, .. } => {
                    for s in streams {
                        if !known(s.relation) {
                            return Err(format!(
                                "phase {i}: unknown stream relation {}",
                                s.relation
                            ));
                        }
                        if !(0.0..=1.0).contains(&s.read_frac) {
                            return Err(format!("phase {i}: bad read_frac"));
                        }
                    }
                }
            }
            if p.len_events() == 0 {
                return Err(format!("phase {i}: zero events"));
            }
        }
        Ok(())
    }

    /// Scale all phase lengths to hit approximately `target` total events.
    pub fn scaled(&self, target: u64) -> WorkloadSpec {
        let total = self.total_events().max(1);
        let f = target as f64 / total as f64;
        let mut out = self.clone();
        for p in out.phases.iter_mut() {
            let scale = |v: &mut u64| {
                *v = ((*v as f64 * f).round() as u64).max(1);
            };
            match p {
                Phase::Zipf { len, .. }
                | Phase::WriteBurst { len, .. }
                | Phase::Mixed { len, .. }
                | Phase::Churn { len, .. }
                | Phase::OltpScan { len, .. } => scale(len),
                Phase::Scan { repeats, .. } => scale(repeats),
            }
        }
        out
    }
}

pub fn load_spec(path: &str) -> Result<WorkloadSpec, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
    let spec: WorkloadSpec =
        serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))?;
    spec.validate()?;
    Ok(spec)
}

struct Gen<'a> {
    rng: Rng,
    spec: &'a WorkloadSpec,
    rel_by_id: HashMap<u32, &'a RelationSpec>,
    first_seen: HashMap<u64, u64>,
    out: Vec<TraceEvent>,
    zipf_cache: HashMap<(u64, u64), Zipf>, // (npages, theta_bits) -> table
}

impl<'a> Gen<'a> {
    fn zipf(&mut self, npages: u64, theta: f64) -> Zipf {
        let key = (npages, theta.to_bits());
        if let Some(z) = self.zipf_cache.get(&key) {
            return z.clone();
        }
        let z = Zipf::new(npages, theta);
        self.zipf_cache.insert(key, z.clone());
        z
    }

    fn push(&mut self, page_id: u64, relation: u32, op: Op, offset: u64) {
        let seq = self.out.len() as u64;
        let creation = *self.first_seen.entry(page_id).or_insert(seq);
        let rel = self.rel_by_id.get(&relation);
        let group = rel.map(|r| (offset / r.group_size) as u32);
        // Occasional Meta page touch (root / catalog analogue): route 0.1%
        // of accesses through the relation's base page as Meta.
        let (page_id, ptype) = if seq % 1000 == 999 {
            let base = rel.map(|r| r.base).unwrap_or(0);
            (base, PageType::Meta)
        } else {
            (page_id, self.page_type_for(relation, offset))
        };
        let _ = group;
        self.out.push(TraceEvent {
            seq,
            page_id,
            relation_id: relation,
            op,
            page_type: ptype,
            txn_id: seq / 8,
            size_bytes: self.spec.page_bytes,
            relation_kind: rel.map(|r| r.kind.clone()),
            logical_group: group,
            creation_time: Some(creation),
        });
    }

    fn page_type_for(&self, relation: u32, offset: u64) -> PageType {
        match self.rel_by_id.get(&relation) {
            Some(r) if offset < r.inner_pages => PageType::Inner,
            Some(r) if r.kind == "blob" => PageType::Blob,
            Some(r) if r.kind == "heap" => PageType::Heap,
            _ => PageType::Leaf,
        }
    }

    fn zipf_pick(&mut self, start: u64, npages: u64, theta: f64) -> (u64, u64) {
        let z = self.zipf(npages, theta);
        let rank = z.sample(&mut self.rng);
        (start + rank, rank)
    }

    fn op(&mut self, read_frac: f64) -> Op {
        if self.rng.bernoulli(read_frac) {
            Op::Read
        } else {
            Op::Write
        }
    }

    fn run_phase(&mut self, phase: &Phase) {
        match phase.clone() {
            Phase::Zipf {
                len,
                start,
                npages,
                theta,
                read_frac,
                relation,
            } => {
                for _ in 0..len {
                    let (page, rank) = self.zipf_pick(start, npages, theta);
                    let op = self.op(read_frac);
                    self.push(page, relation, op, rank);
                }
            }
            Phase::Scan {
                start,
                npages,
                stride,
                repeats,
                read_frac,
                relation,
            } => {
                for _ in 0..repeats {
                    let mut off = 0u64;
                    while off < npages {
                        let op = self.op(read_frac);
                        self.push(start + off, relation, op, off);
                        off += stride.max(1);
                    }
                }
            }
            Phase::WriteBurst {
                len,
                start,
                npages,
                theta,
                base_read_frac,
                burst_write_pages,
                burst_len_frac,
                relation,
            } => {
                let b0 = (len as f64 * (0.5 - burst_len_frac / 2.0)) as u64;
                let b1 = (len as f64 * (0.5 + burst_len_frac / 2.0)) as u64;
                for i in 0..len {
                    if i >= b0 && i < b1 {
                        // Concentrated updates on a small subset.
                        let off = self.rng.below(burst_write_pages.max(1));
                        self.push(start + off, relation, Op::Write, off);
                    } else {
                        let (page, rank) = self.zipf_pick(start, npages, theta);
                        let op = self.op(base_read_frac);
                        self.push(page, relation, op, rank);
                    }
                }
            }
            Phase::Mixed { len, streams } => {
                let total_w: f64 = streams.iter().map(|s| s.weight).sum();
                assert!(total_w > 0.0, "mixed phase with zero weight");
                for _ in 0..len {
                    let mut u = self.rng.below(1 << 53) as f64 / (1u64 << 53) as f64;
                    let mut chosen = &streams[0];
                    for s in &streams {
                        u -= s.weight / total_w;
                        if u <= 0.0 {
                            chosen = s;
                            break;
                        }
                        chosen = s;
                    }
                    let (page, rank) = self.zipf_pick(chosen.start, chosen.npages, chosen.theta);
                    let op = self.op(chosen.read_frac);
                    self.push(page, chosen.relation, op, rank);
                }
            }
            Phase::Churn {
                len,
                pool_start,
                pool_size,
                window,
                theta,
                read_frac,
                relation,
            } => {
                // Sliding hot window: window start advances through the pool;
                // each page is hot for ~life events then dies.
                for i in 0..len {
                    let frac = i as f64 / len.max(1) as f64;
                    let wstart =
                        pool_start + ((pool_size.saturating_sub(window)) as f64 * frac) as u64;
                    let (off, rank) = {
                        let z = self.zipf(window.min(pool_size).max(1), theta);
                        let r = z.sample(&mut self.rng);
                        (r, r)
                    };
                    let op = self.op(read_frac);
                    self.push(wstart + off, relation, op, rank);
                }
            }
            Phase::OltpScan {
                len,
                start,
                npages,
                theta,
                read_frac,
                relation,
                scan_every,
                scan_npages,
                scan_start,
                scan_relation,
            } => {
                let mut since_scan = 0u64;
                let mut scan_off = 0u64;
                for _ in 0..len {
                    since_scan += 1;
                    if since_scan >= scan_every.max(1) {
                        since_scan = 0;
                        for k in 0..scan_npages {
                            let off = (scan_off + k) % scan_npages;
                            self.push(scan_start + off, scan_relation, Op::Read, off);
                        }
                        scan_off = (scan_off + scan_npages) % scan_npages.max(1);
                    } else {
                        let (page, rank) = self.zipf_pick(start, npages, theta);
                        let op = self.op(read_frac);
                        self.push(page, relation, op, rank);
                    }
                }
            }
        }
    }
}

/// Generate a deterministic trace from a spec + seed.
pub fn generate(spec: &WorkloadSpec, seed: u64) -> Vec<TraceEvent> {
    spec.validate().expect("invalid workload spec");
    let mut g = Gen {
        rng: Rng::new(
            seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x1234),
        ),
        spec,
        rel_by_id: spec.relations.iter().map(|r| (r.id, r)).collect(),
        first_seen: HashMap::new(),
        out: Vec::new(),
        zipf_cache: HashMap::new(),
    };
    // Per-phase RNG continuity: one stream per trace (documented). Phase
    // boundaries are deterministic given the seed.
    for phase in spec.phases.clone() {
        g.run_phase(&phase);
    }
    crate::trace::validate_trace(&g.out).expect("generator produced invalid trace");
    g.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_spec() -> WorkloadSpec {
        WorkloadSpec {
            id: "test".into(),
            description: "test".into(),
            universe_pages: 1000,
            page_bytes: 8192,
            relations: vec![RelationSpec {
                id: 0,
                kind: "btree".into(),
                base: 0,
                len: 1000,
                inner_pages: 16,
                group_size: 64,
            }],
            phases: vec![Phase::Zipf {
                len: 500,
                start: 0,
                npages: 200,
                theta: 1.0,
                read_frac: 0.8,
                relation: 0,
            }],
        }
    }

    #[test]
    fn deterministic_across_calls() {
        let s = tiny_spec();
        assert_eq!(generate(&s, 1), generate(&s, 1));
    }

    #[test]
    fn seeds_differ() {
        let s = tiny_spec();
        assert_ne!(generate(&s, 1), generate(&s, 2));
    }

    #[test]
    fn respects_bounds() {
        let s = tiny_spec();
        for ev in generate(&s, 3) {
            assert!(ev.page_id < 200, "page {} out of hot range", ev.page_id);
        }
    }

    #[test]
    fn rejects_bad_spec() {
        let mut s = tiny_spec();
        s.phases = vec![];
        assert!(s.validate().is_err());
    }
}
