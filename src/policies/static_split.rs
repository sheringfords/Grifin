//! Static hot/cold split: the deliberately naive control.
//!
//! Admits pages whose id falls below a fixed threshold (25% of the address
//! space by default) and rejects everything else. No adaptation, no
//! frequency tracking, LRU order within a tier only to pick victims.
//!
//! Purpose: if Grifin cannot beat *this* on phase-change workloads, the
//! mechanism is vacuous. If a strong policy loses to this anywhere, the
//! workload is telling us something about static partitionability.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{BTreeMap, HashMap};

pub struct StaticSplit {
    ctx: PolicyCtx,
    /// Admit pages with `page_id % modulo < admit_mod`.
    modulo: u64,
    admit_mod: u64,
    tick: u64,
    pos: HashMap<PageId, (Tier, u64)>,
    order_t0: BTreeMap<u64, PageId>,
    order_t1: BTreeMap<u64, PageId>,
}

impl StaticSplit {
    pub fn new(ctx: PolicyCtx) -> Self {
        Self {
            ctx,
            modulo: 4,
            admit_mod: 1,
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

impl Policy for StaticSplit {
    fn name(&self) -> &str {
        "static"
    }
    fn config_str(&self) -> String {
        format!("static-split(mod={},admit<{})", self.modulo, self.admit_mod)
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        match resident {
            Some((tier, _)) => {
                self.touch(ev.page_id, tier);
                Decision::bypass()
            }
            None => {
                if ev.page_id % self.modulo < self.admit_mod {
                    let tier = if self.ctx.t0_cap > 0 {
                        Tier::T0
                    } else {
                        Tier::T1
                    };
                    Decision::admit(tier)
                } else {
                    Decision::bypass()
                }
            }
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        match tier {
            Tier::T0 => self.order_t0.values().next().copied(),
            Tier::T1 => self.order_t1.values().next().copied(),
        }
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
        (self.pos.len() as u64).saturating_mul(24)
    }
}
