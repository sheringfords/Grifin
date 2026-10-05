//! Grifin V0: interpretable database-semantic placement (no ML).
//!
//! Value model (frozen for V1; see `docs/EXPERIMENT.md` for the config):
//!
//! ```text
//! value(p) = REUSE_W * ln(1 + freq(p))
//!          + WRITE_W * write_frac(p) + DIRTY_B * is_dirty   [use_write]
//!          + REL_W   * ln(1 + rel_freq(r)) / 4               [use_relation]
//!          - SCAN_P  (if relation r is in sequential-scan mode) [use_relation]
//!          - NEW_P   (if one-hit-wonder and relation cold)  [use_lifetime]
//! ```
//!
//! Signals:
//! - reuse: decayed per-page frequency (lazy epoch halving; recency is
//!   implicit — stale pages decay). Always on.
//! - write: per-page write fraction + dirty bonus. Reads cost latency but
//!   writes cost latency AND endurance; retaining soon-rewritten dirty pages
//!   in fast memory avoids storage writebacks.
//! - relation: per-relation decayed temperature (relation identity is the
//!   cheapest genuine DB semantic: pages of a hot index share fate) plus
//!   sequential-scan detection (stride-1 runs >= SCAN_RUN) whose pages are
//!   devalued so scans do not flush the hot set.
//! - lifetime: pages seen exactly once within LIFETIME_WINDOW events are
//!   devalued (short-lived-object resistance), unless their relation is hot.
//!
//! Admission: value >= TH_T0 -> T0; >= TH_T1 -> T1; else bypass. Victim =
//! minimum value in the tier. T1 hits with value >= TH_T0 promote.
//!
//! Ablations (feature flags): reuse-only, +write, +relation, full. The flag
//! set is part of the policy name so results cannot mix them up.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::rng::Rng;
use crate::trace::{PageId, TraceEvent};
use std::collections::HashMap;

pub const REUSE_W: f64 = 1.0;
pub const WRITE_W: f64 = 0.8;
pub const DIRTY_B: f64 = 0.5;
pub const REL_W: f64 = 0.6;
pub const SCAN_P: f64 = 3.0;
pub const NEW_P: f64 = 1.5;
pub const TH_T0: f64 = 2.4;
pub const TH_T1: f64 = 0.2;
pub const SCAN_RUN: u32 = 24;
pub const LIFETIME_WINDOW: u64 = 512;
pub const EPOCH_LEN: u64 = 4096;
pub const DECAY_SHIFT_CAP: u32 = 4;
/// Victim sampling width. Sweeping a whole tier per eviction is O(n); V0
/// samples K residents and evicts the minimum-value one — the same
/// sampling-based replacement LeanStore's LeanEvict and WATT use for
/// scalability (documented; see literature.md). Deterministic via own RNG.
pub const VICTIM_SAMPLES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct GrifinFlags {
    pub use_write: bool,
    pub use_relation: bool,
    pub use_lifetime: bool,
}

impl GrifinFlags {
    pub fn reuse_only() -> Self {
        Self {
            use_write: false,
            use_relation: false,
            use_lifetime: false,
        }
    }
    pub fn reuse_write() -> Self {
        Self {
            use_write: true,
            use_relation: false,
            use_lifetime: false,
        }
    }
    pub fn reuse_relation() -> Self {
        Self {
            use_write: false,
            use_relation: true,
            use_lifetime: false,
        }
    }
    pub fn full() -> Self {
        Self {
            use_write: true,
            use_relation: true,
            use_lifetime: true,
        }
    }
    pub fn tag(&self) -> &'static str {
        match (self.use_write, self.use_relation, self.use_lifetime) {
            (false, false, false) => "reuse",
            (true, false, false) => "reuse-write",
            (false, true, false) => "reuse-relation",
            (true, true, true) => "full",
            _ => "custom",
        }
    }
}

#[derive(Clone, Debug)]
struct PageStat {
    count: u32,
    epoch: u32,
    writes: u32,
    first: u64,
    last: u64,
    relation: u32,
}

#[derive(Clone, Debug, Default)]
struct RelStat {
    count: u32,
    epoch: u32,
    last_page: u64,
    has_last: bool,
    run: u32,
    scan: bool,
}

pub struct Grifin {
    ctx: PolicyCtx,
    flags: GrifinFlags,
    now: u64,
    epoch: u32,
    pages: HashMap<PageId, PageStat>,
    rels: HashMap<u32, RelStat>,
    resident: HashMap<PageId, Tier>,
    /// Per-tier membership lists for O(1) victim sampling.
    members_t0: Vec<PageId>,
    idx_t0: HashMap<PageId, usize>,
    members_t1: Vec<PageId>,
    idx_t1: HashMap<PageId, usize>,
    rng: Rng,
}

impl Grifin {
    pub fn new(ctx: PolicyCtx, flags: GrifinFlags) -> Self {
        Self {
            ctx,
            flags,
            now: 0,
            epoch: 0,
            pages: HashMap::new(),
            rels: HashMap::new(),
            resident: HashMap::new(),
            members_t0: Vec::new(),
            idx_t0: HashMap::new(),
            members_t1: Vec::new(),
            idx_t1: HashMap::new(),
            rng: Rng::new(0xC11F_1E5E_0000_0001),
        }
    }

    fn eff_count(count: u32, epoch: u32, now_epoch: u32) -> f64 {
        let shift = (now_epoch.saturating_sub(epoch)).min(DECAY_SHIFT_CAP);
        ((count >> shift) as f64).max(if count > 0 { 0.5 } else { 0.0 })
    }

    fn touch(&mut self, ev: &TraceEvent) {
        self.now += 1;
        if self.now.is_multiple_of(EPOCH_LEN) {
            self.epoch += 1;
        }
        let epoch = self.epoch;
        let st = self.pages.entry(ev.page_id).or_insert(PageStat {
            count: 0,
            epoch,
            writes: 0,
            first: self.now,
            last: 0,
            relation: ev.relation_id,
        });
        // Refresh decay baseline without losing hot counts: fold at most one
        // shift per epoch crossing (lazy decay in eff_count handles the rest).
        if st.epoch != epoch {
            let shift = (epoch - st.epoch).min(DECAY_SHIFT_CAP);
            st.count >>= shift;
            st.writes >>= shift.min(2);
            st.epoch = epoch;
        }
        st.count = st.count.saturating_add(1);
        if ev.op.is_write() {
            st.writes = st.writes.saturating_add(1);
        }
        st.last = self.now;

        if self.flags.use_relation {
            let rs = self.rels.entry(ev.relation_id).or_default();
            if rs.epoch != epoch {
                let shift = (epoch - rs.epoch).min(DECAY_SHIFT_CAP);
                rs.count >>= shift;
                rs.epoch = epoch;
            }
            rs.count = rs.count.saturating_add(1);
            // Sequential-scan detection: stride-1 runs within a relation.
            if rs.has_last && ev.page_id == rs.last_page.wrapping_add(1) {
                rs.run += 1;
            } else {
                rs.run = 0;
                rs.scan = false;
            }
            if rs.run >= SCAN_RUN {
                rs.scan = true;
            }
            rs.last_page = ev.page_id;
            rs.has_last = true;
        }
    }

    fn value(&self, page: PageId, dirty: bool) -> f64 {
        let Some(st) = self.pages.get(&page) else {
            return 0.0;
        };
        let f = Self::eff_count(st.count, st.epoch, self.epoch);
        let mut v = REUSE_W * (1.0 + f).ln();
        if self.flags.use_write {
            let wf = st.writes as f64 / (1.0 + f);
            v += WRITE_W * wf;
            if dirty {
                v += DIRTY_B;
            }
        }
        if self.flags.use_relation {
            if let Some(rs) = self.rels.get(&st.relation) {
                let rf = Self::eff_count(rs.count, rs.epoch, self.epoch);
                v += REL_W * (1.0 + rf).ln() / 4.0;
                if rs.scan {
                    v -= SCAN_P;
                }
                if self.flags.use_lifetime
                    && f <= 1.0
                    && self.now.saturating_sub(st.first) < LIFETIME_WINDOW
                    && rf < 8.0
                {
                    v -= NEW_P;
                }
            }
        } else if self.flags.use_lifetime
            && f <= 1.0
            && self.now.saturating_sub(st.first) < LIFETIME_WINDOW
        {
            // Without relation identity there is no "hot relation" escape
            // hatch; the penalty still applies (ablation honesty).
            v -= NEW_P;
        }
        v
    }

    fn admit_tier(&self, page: PageId) -> Option<Tier> {
        let dirty = false; // missed pages are not dirty yet
        let v = self.value(page, dirty);
        if v >= TH_T0 && self.ctx.t0_cap > 0 {
            Some(Tier::T0)
        } else if v >= TH_T1 {
            if self.ctx.t1_cap > 0 {
                Some(Tier::T1)
            } else if self.ctx.t0_cap > 0 {
                Some(Tier::T0)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn members_add(&mut self, page: PageId, tier: Tier) {
        let (members, idx) = match tier {
            Tier::T0 => (&mut self.members_t0, &mut self.idx_t0),
            Tier::T1 => (&mut self.members_t1, &mut self.idx_t1),
        };
        debug_assert!(!idx.contains_key(&page));
        idx.insert(page, members.len());
        members.push(page);
    }

    fn members_remove(&mut self, page: PageId, tier: Tier) {
        let (members, idx) = match tier {
            Tier::T0 => (&mut self.members_t0, &mut self.idx_t0),
            Tier::T1 => (&mut self.members_t1, &mut self.idx_t1),
        };
        if let Some(i) = idx.remove(&page) {
            let last = members.len() - 1;
            if i != last {
                let moved = members[last];
                members[i] = moved;
                idx.insert(moved, i);
            }
            members.pop();
        }
    }
}

impl Policy for Grifin {
    fn name(&self) -> &str {
        // Static tag would be nicer; return a leaked static per flag combo.
        // Names: grifin-reuse | grifin-reuse-write | grifin-reuse-relation | grifin-full
        match (
            self.flags.use_write,
            self.flags.use_relation,
            self.flags.use_lifetime,
        ) {
            (false, false, false) => "grifin-reuse",
            (true, false, false) => "grifin-reuse-write",
            (false, true, false) => "grifin-reuse-relation",
            (true, true, true) => "grifin-full",
            _ => "grifin-custom",
        }
    }

    fn config_str(&self) -> String {
        format!(
            "grifin({tag},REUSE_W={REUSE_W},WRITE_W={WRITE_W},DIRTY_B={DIRTY_B},\
             REL_W={REL_W},SCAN_P={SCAN_P},NEW_P={NEW_P},TH_T0={TH_T0},TH_T1={TH_T1},\
             SCAN_RUN={SCAN_RUN},LIFETIME_WINDOW={LIFETIME_WINDOW},EPOCH_LEN={EPOCH_LEN})",
            tag = self.flags.tag(),
        )
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        self.touch(ev);
        match resident {
            Some((tier, dirty)) => {
                let v = self.value(ev.page_id, dirty);
                Decision {
                    admit: None,
                    promote: tier == Tier::T1 && v >= TH_T0,
                }
            }
            None => match self.admit_tier(ev.page_id) {
                Some(t) => Decision::admit(t),
                None => Decision::bypass(),
            },
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        // Sampling-based replacement: examine VICTIM_SAMPLES random
        // residents, evict the minimum value (ties -> smaller page id).
        let n = match tier {
            Tier::T0 => self.members_t0.len(),
            Tier::T1 => self.members_t1.len(),
        };
        if n == 0 {
            return None;
        }
        // Draw indices first (mutable rng), evaluate after (immutable
        // member lists) to keep borrows disjoint.
        let k = VICTIM_SAMPLES.min(n);
        let mut idxs = [0usize; VICTIM_SAMPLES];
        for s in idxs.iter_mut().take(k) {
            *s = self.rng.below(n as u64) as usize;
        }
        let members = match tier {
            Tier::T0 => &self.members_t0,
            Tier::T1 => &self.members_t1,
        };
        let mut best: Option<(PageId, f64)> = None;
        for s in idxs.iter().take(k) {
            let p = members[*s];
            let v = self.value(p, false);
            let take = match best {
                None => true,
                Some((bp, bv)) => v < bv || (v == bv && p < bp),
            };
            if take {
                best = Some((p, v));
            }
        }
        best.map(|(p, _)| p)
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        self.resident.insert(page, tier);
        self.members_add(page, tier);
    }

    fn on_move(&mut self, page: PageId, from: Tier, to: Tier) {
        self.resident.insert(page, to);
        self.members_remove(page, from);
        self.members_add(page, to);
    }

    fn on_evict(&mut self, page: PageId, tier: Tier) {
        self.resident.remove(&page);
        self.members_remove(page, tier);
    }

    fn tracked(&self) -> usize {
        self.resident.len()
    }

    fn metadata_bytes(&self) -> u64 {
        // ~64B per tracked page + ~48B per relation (estimate, labelled).
        (self.pages.len() as u64)
            .saturating_mul(64)
            .saturating_add((self.rels.len() as u64).saturating_mul(48))
    }
}
