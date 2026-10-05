//! Per-tier LRU: the weak-but-honest control.
//!
//! New misses go to T0 (or T1 if T0 has zero capacity); T1 hits promote.
//! Reference: no single paper; LRU is the textbook baseline. O(log n) via
//! recency-ordered maps.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{BTreeMap, HashMap};

pub struct Lru {
    ctx: PolicyCtx,
    tick: u64,
    /// page -> (tier, tick)
    pos: HashMap<PageId, (Tier, u64)>,
    order_t0: BTreeMap<u64, PageId>,
    order_t1: BTreeMap<u64, PageId>,
}

impl Lru {
    pub fn new(ctx: PolicyCtx) -> Self {
        Self {
            ctx,
            tick: 0,
            pos: HashMap::new(),
            order_t0: BTreeMap::new(),
            order_t1: BTreeMap::new(),
        }
    }

    fn touch(&mut self, page: PageId, tier: Tier) {
        if let Some((old_tier, old_tick)) = self.pos.get(&page).copied() {
            match old_tier {
                Tier::T0 => self.order_t0.remove(&old_tick),
                Tier::T1 => self.order_t1.remove(&old_tick),
            };
        }
        self.tick += 1;
        self.pos.insert(page, (tier, self.tick));
        match tier {
            Tier::T0 => self.order_t0.insert(self.tick, page),
            Tier::T1 => self.order_t1.insert(self.tick, page),
        };
    }

    fn remove(&mut self, page: PageId) {
        if let Some((tier, tick)) = self.pos.remove(&page) {
            match tier {
                Tier::T0 => self.order_t0.remove(&tick),
                Tier::T1 => self.order_t1.remove(&tick),
            };
        }
    }
}

impl Policy for Lru {
    fn name(&self) -> &str {
        "lru"
    }
    fn config_str(&self) -> String {
        "per-tier-lru".to_string()
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        match resident {
            Some((tier, _)) => {
                self.touch(ev.page_id, tier);
                Decision {
                    admit: None,
                    promote: tier == Tier::T1,
                }
            }
            None => {
                let tier = if self.ctx.t0_cap > 0 {
                    Tier::T0
                } else {
                    Tier::T1
                };
                Decision::admit(tier)
            }
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        let map = match tier {
            Tier::T0 => &self.order_t0,
            Tier::T1 => &self.order_t1,
        };
        map.values().next().copied()
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        self.touch(page, tier);
    }

    fn on_move(&mut self, page: PageId, _from: Tier, to: Tier) {
        self.touch(page, to);
    }

    fn on_evict(&mut self, page: PageId, _tier: Tier) {
        self.remove(page);
    }

    fn tracked(&self) -> usize {
        self.pos.len()
    }

    fn metadata_bytes(&self) -> u64 {
        // ~24 bytes per resident entry (map node + tick), both tiers.
        (self.pos.len() as u64).saturating_mul(24)
    }
}
