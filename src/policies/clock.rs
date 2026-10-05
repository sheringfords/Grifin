//! Per-tier CLOCK: the classic low-overhead LRU approximation.
//!
//! Each tier is a circular buffer of (page, refbit). Hits set the refbit;
//! victims are found by sweeping the hand, clearing refbits. CLOCK is the
//! policy PostgreSQL's clock-sweep buffer replacement descends from, which
//! makes it the historically honest "production-like" control.
//!
//! Reference mechanism: Corbato-style CLOCK (multilevel feedback via the
//! reference bit); see e.g. Jiang/Chen/Zhang, USENIX ATC '05 for background
//! and the known weak-locality limitations.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::HashMap;

#[derive(Clone)]
struct Slot {
    page: PageId,
    refbit: bool,
}

struct ClockTier {
    slots: Vec<Slot>,
    index: HashMap<PageId, usize>,
    hand: usize,
}

impl ClockTier {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            index: HashMap::new(),
            hand: 0,
        }
    }

    fn touch(&mut self, page: PageId) {
        if let Some(&i) = self.index.get(&page) {
            self.slots[i].refbit = true;
        }
    }

    fn insert(&mut self, page: PageId) {
        debug_assert!(!self.index.contains_key(&page));
        self.index.insert(page, self.slots.len());
        self.slots.push(Slot { page, refbit: true });
    }

    /// Remove slot i via swap-remove, fixing the moved index.
    fn remove_at(&mut self, i: usize) -> PageId {
        let last = self.slots.len() - 1;
        let victim = self.slots[i].page;
        self.index.remove(&victim);
        if i != last {
            let moved = self.slots[last].page;
            self.slots[i] = self.slots[last].clone();
            self.index.insert(moved, i);
        }
        self.slots.pop();
        if !self.slots.is_empty() {
            self.hand %= self.slots.len();
        } else {
            self.hand = 0;
        }
        victim
    }

    fn remove(&mut self, page: PageId) {
        if let Some(&i) = self.index.get(&page) {
            self.remove_at(i);
        }
    }

    /// CLOCK victim search. Returns None only if the tier is empty.
    fn victim(&mut self) -> Option<PageId> {
        if self.slots.is_empty() {
            return None;
        }
        loop {
            self.hand %= self.slots.len();
            if !self.slots[self.hand].refbit {
                let i = self.hand;
                self.hand = (self.hand + 1) % self.slots.len();
                return Some(self.slots[i].page);
            }
            self.slots[self.hand].refbit = false;
            self.hand = (self.hand + 1) % self.slots.len();
        }
    }
}

pub struct Clock {
    ctx: PolicyCtx,
    t0: ClockTier,
    t1: ClockTier,
}

impl Clock {
    pub fn new(ctx: PolicyCtx) -> Self {
        Self {
            ctx,
            t0: ClockTier::new(),
            t1: ClockTier::new(),
        }
    }

    fn tier_mut(&mut self, t: Tier) -> &mut ClockTier {
        match t {
            Tier::T0 => &mut self.t0,
            Tier::T1 => &mut self.t1,
        }
    }
}

impl Policy for Clock {
    fn name(&self) -> &str {
        "clock"
    }
    fn config_str(&self) -> String {
        "per-tier-clock".to_string()
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        match resident {
            Some((tier, _)) => {
                self.tier_mut(tier).touch(ev.page_id);
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
        self.tier_mut(tier).victim()
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        self.tier_mut(tier).insert(page);
    }

    fn on_move(&mut self, page: PageId, from: Tier, to: Tier) {
        self.tier_mut(from).remove(page);
        self.tier_mut(to).insert(page);
        self.tier_mut(to).touch(page);
    }

    fn on_evict(&mut self, page: PageId, tier: Tier) {
        self.tier_mut(tier).remove(page);
    }

    fn tracked(&self) -> usize {
        self.t0.slots.len() + self.t1.slots.len()
    }

    fn metadata_bytes(&self) -> u64 {
        ((self.t0.slots.len() + self.t1.slots.len()) as u64).saturating_mul(16)
    }
}
