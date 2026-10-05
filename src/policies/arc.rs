//! ARC: Adaptive Replacement Cache (Megiddo & Modha, FAST '03).
//!
//! Textbook ARC over the *combined* resident set (capacity `c = t0+t1`),
//! with one documented two-tier adaptation: list membership follows the
//! paper exactly (T1 = recency, T2 = frequency, B1/B2 = ghosts, adaptive
//! `p`), while tier placement maps T2->T0 (hot) and T1->T1 (probation).
//!
//! Deviation (forced by the simulator contract, which owns eviction timing):
//! textbook REPLACE evicts a combined-cache victim at request time; here
//! the simulator calls `victim(tier)` when a tier is full, and ARC returns
//! its deepest list-order page resident in that tier (T1-list tail, then T2
//! tail). Ghost transitions happen in `on_evict` (simulator-confirmed), and
//! ghost-cap enforcement (`|B1|<=c`, `|B2|<=c`, total directory `<= 2c`) is
//! done lazily on insert. With `t1_cap == 0` this reduces to exact
//! single-level ARC, which the hand-computable tests verify.
//!
//! All operations are O(log n) via a single indexed order map keyed by
//! (list, tier, tick).

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum List {
    T1,
    T2,
    B1,
    B2,
}

impl List {
    fn rank(self) -> u8 {
        match self {
            List::T1 => 0,
            List::T2 => 1,
            List::B1 => 2,
            List::B2 => 3,
        }
    }
    fn of_rank(r: u8) -> Option<List> {
        match r {
            0 => Some(List::T1),
            1 => Some(List::T2),
            2 => Some(List::B1),
            3 => Some(List::B2),
            _ => None,
        }
    }
}

fn tier_rank(tier: Option<Tier>) -> u8 {
    match tier {
        Some(Tier::T0) => 0,
        Some(Tier::T1) => 1,
        None => 2,
    }
}

pub struct Arc {
    ctx: PolicyCtx,
    c: usize,
    p: usize,
    tick: u64,
    /// (list_rank, tier_rank, tick) -> page. Single index, O(log n) all ops.
    ord: BTreeMap<(u8, u8, u64), PageId>,
    rev: HashMap<PageId, (u8, u8, u64)>,
    lens: [usize; 4],
    resident: HashMap<PageId, Tier>,
    /// Pages whose admission was decided but not yet confirmed via on_place.
    pending: HashMap<PageId, List>,
}

impl Arc {
    pub fn new(ctx: PolicyCtx) -> Self {
        let c = ctx.t0_cap + ctx.t1_cap;
        assert!(c > 0, "ARC needs nonzero combined capacity");
        Self {
            ctx,
            c,
            p: 0,
            tick: 0,
            ord: BTreeMap::new(),
            rev: HashMap::new(),
            lens: [0; 4],
            resident: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    fn len(&self, list: List) -> usize {
        self.lens[list.rank() as usize]
    }

    fn detach(&mut self, page: PageId) {
        if let Some(key) = self.rev.remove(&page) {
            self.ord.remove(&key);
            self.lens[key.0 as usize] -= 1;
        }
    }

    /// tick=None assigns a fresh recency tick; Some(t) preserves order
    /// (used when only the tier changes).
    fn attach(&mut self, page: PageId, list: List, tier: Option<Tier>, tick: Option<u64>) {
        self.detach(page);
        let t = match tick {
            Some(t) => t,
            None => {
                self.tick += 1;
                self.tick
            }
        };
        let key = (list.rank(), tier_rank(tier), t);
        self.ord.insert(key, page);
        self.rev.insert(page, key);
        self.lens[list.rank() as usize] += 1;
    }

    fn list_of(&self, page: PageId) -> Option<List> {
        self.rev.get(&page).and_then(|k| List::of_rank(k.0))
    }

    /// Oldest (LRU-end) page of `list`, if any. O(log n).
    fn lru_of(&self, list: List) -> Option<PageId> {
        let lr = list.rank();
        self.ord
            .range((
                Bound::Included((lr, 0, 0)),
                Bound::Included((lr, 2, u64::MAX)),
            ))
            .next()
            .map(|(_, &p)| p)
    }

    /// Deepest ARC-order page resident in `tier`. O(log n).
    fn tier_victim(&self, tier: Tier) -> Option<PageId> {
        let tr = tier_rank(Some(tier));
        for lr in [0u8, 1u8] {
            if let Some((_, &p)) = self
                .ord
                .range((
                    Bound::Included((lr, tr, 0)),
                    Bound::Included((lr, tr, u64::MAX)),
                ))
                .next()
            {
                return Some(p);
            }
        }
        None
    }

    fn delete_ghost_lru(&mut self, list: List) {
        if let Some(p) = self.lru_of(list) {
            self.detach(p);
        }
    }

    /// Keep directory within textbook bounds: |B1|<=c, |B2|<=c, total<=2c,
    /// and |T1|+|B1|<=c (the subcase-A invariant).
    fn enforce_ghost_caps(&mut self) {
        while self.len(List::B1) > self.c {
            self.delete_ghost_lru(List::B1);
        }
        while self.len(List::B2) > self.c {
            self.delete_ghost_lru(List::B2);
        }
        while self.len(List::T1) + self.len(List::B1) > self.c && self.len(List::B1) > 0 {
            self.delete_ghost_lru(List::B1);
        }
        while self.len(List::B1) + self.len(List::B2) > 2 * self.c {
            if self.len(List::B2) == 0 {
                self.delete_ghost_lru(List::B1);
            } else {
                self.delete_ghost_lru(List::B2);
            }
        }
    }
}

impl Policy for Arc {
    fn name(&self) -> &str {
        "arc"
    }
    fn config_str(&self) -> String {
        format!("arc(c={},two-tier:T2->T0,T1->T1)", self.c)
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        let page = ev.page_id;
        match self.list_of(page) {
            Some(List::T1) | Some(List::T2) => {
                // Case I: hit. Graduate to T2 MRU, tier unchanged.
                let was_t1 = self.list_of(page) == Some(List::T1);
                let tier = resident.map(|(t, _)| t);
                self.attach(page, List::T2, tier, None);
                Decision {
                    admit: None,
                    promote: was_t1 && matches!(resident, Some((Tier::T1, _))),
                }
            }
            Some(List::B1) => {
                // Case II: ghost hit in B1 -> increase p, re-enter as T2.
                let b1 = self.len(List::B1).max(1);
                let b2 = self.len(List::B2).max(1);
                let delta = (b2 / b1).max(1);
                self.p = (self.p + delta).min(self.c);
                self.pending.insert(page, List::T2);
                Decision::admit(Tier::T0)
            }
            Some(List::B2) => {
                // Case III: ghost hit in B2 -> decrease p, re-enter as T2.
                let b1 = self.len(List::B1).max(1);
                let b2 = self.len(List::B2).max(1);
                let delta = (b1 / b2).max(1);
                self.p = self.p.saturating_sub(delta);
                self.pending.insert(page, List::T2);
                Decision::admit(Tier::T0)
            }
            None => {
                // Case IV: brand-new page -> T1 list, probation tier.
                self.pending.insert(page, List::T1);
                let tier = if self.ctx.t1_cap > 0 {
                    Tier::T1
                } else {
                    Tier::T0
                };
                Decision::admit(tier)
            }
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        self.tier_victim(tier)
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        let dest = self.pending.remove(&page).unwrap_or(List::T1);
        self.attach(page, dest, Some(tier), None);
        self.resident.insert(page, tier);
        self.enforce_ghost_caps();
    }

    fn on_move(&mut self, page: PageId, _from: Tier, to: Tier) {
        // Tier changes; list membership and recency tick preserved.
        if let Some(list) = self.list_of(page) {
            if matches!(list, List::T1 | List::T2) {
                let tick = self.rev.get(&page).map(|k| k.2);
                self.attach(page, list, Some(to), tick);
            }
        }
        self.resident.insert(page, to);
    }

    fn on_evict(&mut self, page: PageId, _tier: Tier) {
        // Textbook REPLACE movement, tier-restricted: T1-list -> B1, else B2.
        let ghost = match self.list_of(page) {
            Some(List::T1) => List::B1,
            _ => List::B2,
        };
        self.attach(page, ghost, None, None);
        self.resident.remove(&page);
        self.enforce_ghost_caps();
    }

    fn tracked(&self) -> usize {
        self.resident.len()
    }

    fn metadata_bytes(&self) -> u64 {
        (self.rev.len() as u64).saturating_mul(32)
    }
}
