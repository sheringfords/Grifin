//! Simplified LIRS (Jiang & Zhang, SIGMETRICS '02).
//!
//! Mechanism: blocks are classified by Inter-Reference Recency (IRR) into
//! LIR (low IRR, protected) and HIR (high IRR, replaceable) sets. A recency
//! stack S records all tracked blocks; resident HIRs additionally sit in a
//! FIFO queue Q; the stack bottom is pruned of HIRs. Re-referencing a
//! bottom-of-stack HIR switches its status with the bottom LIR.
//!
//! Two-tier mapping: LIR->T0, HIR->T1 (everything to T0 in single-tier
//! mode). Documented deviations from the paper: (a) bootstrap rule — HIR
//! hits become LIR while `|LIR| < L` (standard in most reimplementations,
//! since 1:1 status switches alone can never populate the set); (b) the
//! simulator owns eviction timing, so Q-head eviction is served through
//! `victim()` restricted to the requested tier, preferring resident HIRs
//! and only then the stack-deepest LIR (a forced demotion, which switches
//! that LIR to non-resident HIR); (c) Q is recency-ordered (hits move to
//! the tail) rather than pure FIFO.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    Lir,
    HirRes,
    HirNonRes,
}

pub struct Lirs {
    ctx: PolicyCtx,
    /// Target LIR-set size (= fast-tier capacity, or combined in 1-tier mode).
    l_target: usize,
    tick: u64,
    qtick: u64,
    /// Stack S: recency order of all tracked blocks.
    stack: BTreeMap<u64, PageId>,
    stack_pos: HashMap<PageId, u64>,
    /// Resident-HIR queue keyed by (tier_rank, recency) for O(log n)
    /// tier-restricted victim selection.
    q: BTreeMap<(u8, u64), PageId>,
    q_pos: HashMap<PageId, (u8, u64)>,
    status: HashMap<PageId, Status>,
    resident: HashMap<PageId, Tier>,
    pending: HashMap<PageId, Status>,
}

impl Lirs {
    pub fn new(ctx: PolicyCtx) -> Self {
        let combined = ctx.t0_cap + ctx.t1_cap;
        assert!(combined > 0, "LIRS needs nonzero combined capacity");
        let l_target = if ctx.t1_cap > 0 {
            ctx.t0_cap.max(1)
        } else {
            combined
        };
        Self {
            ctx,
            l_target,
            tick: 0,
            qtick: 0,
            stack: BTreeMap::new(),
            stack_pos: HashMap::new(),
            q: BTreeMap::new(),
            q_pos: HashMap::new(),
            status: HashMap::new(),
            resident: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    fn lir_count(&self) -> usize {
        self.status.values().filter(|s| **s == Status::Lir).count()
    }

    fn stack_top(&mut self, page: PageId) {
        if let Some(t) = self.stack_pos.remove(&page) {
            self.stack.remove(&t);
        }
        self.tick += 1;
        self.stack.insert(self.tick, page);
        self.stack_pos.insert(page, self.tick);
    }

    fn stack_bottom(&self) -> Option<PageId> {
        self.stack.values().next().copied()
    }

    /// Remove HIR entries from the stack bottom until an LIR is at the
    /// bottom (or the stack is empty). Evicted-from-stack HIRs that are not
    /// resident become untracked.
    fn prune(&mut self) {
        while let Some(&bottom) = self.stack.values().next() {
            match self.status.get(&bottom) {
                Some(Status::Lir) => break,
                _ => {
                    let t = self.stack_pos.remove(&bottom).unwrap();
                    self.stack.remove(&t);
                    if !self.resident.contains_key(&bottom) {
                        self.status.remove(&bottom);
                    } else {
                        // Resident HIR pruned from stack stays HIR-resident.
                        debug_assert_eq!(self.status.get(&bottom), Some(&Status::HirRes));
                    }
                }
            }
        }
    }

    fn bottom_lir(&self) -> Option<PageId> {
        // Deepest (oldest) LIR in the stack.
        for page in self.stack.values() {
            if self.status.get(page) == Some(&Status::Lir) {
                return Some(*page);
            }
        }
        None
    }

    fn q_push(&mut self, page: PageId) {
        if let Some(k) = self.q_pos.remove(&page) {
            self.q.remove(&k);
        }
        // Q holds resident HIRs; tier from the residency mirror.
        let tr = match self.resident.get(&page) {
            Some(Tier::T0) => 0u8,
            _ => 1u8,
        };
        self.qtick += 1;
        let key = (tr, self.qtick);
        self.q.insert(key, page);
        self.q_pos.insert(page, key);
    }

    fn q_remove(&mut self, page: PageId) {
        if let Some(k) = self.q_pos.remove(&page) {
            self.q.remove(&k);
        }
    }

    /// Refresh Q position after a tier change (promotion/demotion).
    fn q_retier(&mut self, page: PageId) {
        if self.status.get(&page) == Some(&Status::HirRes) {
            self.q_push(page);
        }
    }

    fn set_status(&mut self, page: PageId, s: Status) {
        match s {
            Status::HirRes => self.q_push(page),
            _ => self.q_remove(page),
        }
        self.status.insert(page, s);
    }

    /// Status-switch: `hir_page` (HIR at stack bottom, just re-referenced)
    /// becomes LIR; the deepest LIR becomes resident HIR.
    fn switch_bottom(&mut self, hir_page: PageId) {
        if let Some(lir) = self.bottom_lir() {
            if lir != hir_page {
                self.set_status(hir_page, Status::Lir);
                self.set_status(lir, Status::HirRes);
            }
        } else {
            // No LIR yet (bootstrap): HIR simply becomes LIR.
            self.set_status(hir_page, Status::Lir);
        }
        self.prune();
    }

    fn tier_for(&self, s: Status) -> Tier {
        match s {
            Status::Lir => Tier::T0,
            _ => {
                if self.ctx.t1_cap > 0 {
                    Tier::T1
                } else {
                    Tier::T0
                }
            }
        }
    }

    /// Oldest Q entry resident in `tier`. Skips stale entries defensively.
    fn q_victim_in(&mut self, tier: Tier) -> Option<PageId> {
        let tr = match tier {
            Tier::T0 => 0u8,
            Tier::T1 => 1u8,
        };
        // Collect stale keys to drop after the scan (borrow discipline).
        let mut stale: Vec<(u8, u64)> = Vec::new();
        let mut found = None;
        for (k, page) in self.q.range((tr, 0)..=(tr, u64::MAX)) {
            if self.resident.get(page) == Some(&tier)
                && self.status.get(page) == Some(&Status::HirRes)
            {
                found = Some(*page);
                break;
            }
            stale.push(*k);
        }
        for k in stale {
            if let Some(p) = self.q.remove(&k) {
                self.q_pos.remove(&p);
            }
        }
        found
    }

    /// Stack-deepest resident (any status) in `tier`, for forced LIR demotion.
    fn deepest_resident_in(&self, tier: Tier) -> Option<PageId> {
        for page in self.stack.values() {
            if self.resident.get(page) == Some(&tier) {
                return Some(*page);
            }
        }
        // Resident but pruned from stack (resident HIR not in S): any match.
        self.resident
            .iter()
            .filter(|(_, &t)| t == tier)
            .map(|(&p, _)| p)
            .next()
    }
}

impl Policy for Lirs {
    fn name(&self) -> &str {
        "lirs"
    }
    fn config_str(&self) -> String {
        format!("lirs(L={},LIR->T0,HIR->T1)", self.l_target)
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        let page = ev.page_id;
        match self.status.get(&page).copied() {
            Some(Status::Lir) => {
                self.stack_top(page);
                self.prune();
                Decision {
                    admit: None,
                    promote: matches!(resident, Some((Tier::T1, _))),
                }
            }
            Some(Status::HirRes) => {
                let was_bottom = self.stack_bottom() == Some(page);
                self.stack_top(page);
                self.q_push(page);
                if was_bottom {
                    self.switch_bottom(page);
                } else if self.lir_count() < self.l_target
                    && self.status.get(&page) == Some(&Status::HirRes)
                {
                    // Bootstrap: grow the LIR set without a switch partner.
                    self.set_status(page, Status::Lir);
                }
                self.prune();
                let now_lir = self.status.get(&page) == Some(&Status::Lir);
                Decision {
                    admit: None,
                    promote: now_lir && matches!(resident, Some((Tier::T1, _))),
                }
            }
            // Tracked but not resident, or entirely new.
            _ => {
                let tracked_hir = self.status.get(&page) == Some(&Status::HirNonRes);
                if tracked_hir {
                    // Non-resident HIR hit: textbook evicts Q head and
                    // switches this page to LIR. The physical eviction is the
                    // simulator's job (victim()); record the promotion here.
                    let was_bottom = self.stack_bottom() == Some(page);
                    self.stack_top(page);
                    if was_bottom || self.lir_count() < self.l_target {
                        self.switch_bottom(page);
                    }
                    self.pending.insert(
                        page,
                        if self.status.get(&page) == Some(&Status::Lir) {
                            Status::Lir
                        } else {
                            Status::HirRes
                        },
                    );
                    let st = self.pending[&page];
                    Decision::admit(self.tier_for(st))
                } else {
                    // Brand-new page: enters as resident HIR.
                    self.stack_top(page);
                    self.pending.insert(page, Status::HirRes);
                    Decision::admit(self.tier_for(Status::HirRes))
                }
            }
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        // Prefer the oldest resident HIR in the tier (Q order); only if the
        // tier holds no HIR at all, force-demote the stack-deepest LIR
        // (status switch happens in on_evict).
        if let Some(v) = self.q_victim_in(tier) {
            return Some(v);
        }
        self.deepest_resident_in(tier)
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        let st = self.pending.remove(&page).unwrap_or(Status::HirRes);
        self.stack_top(page);
        // Residency first: set_status -> q_push reads the tier mirror.
        self.resident.insert(page, tier);
        self.set_status(page, st);
        self.prune();
    }

    fn on_move(&mut self, page: PageId, _from: Tier, to: Tier) {
        self.resident.insert(page, to);
        self.q_retier(page);
    }

    fn on_evict(&mut self, page: PageId, _tier: Tier) {
        self.resident.remove(&page);
        self.q_remove(page);
        // Forced LIR demotions become non-resident HIRs (kept in the stack
        // so re-access is a non-resident HIR hit); HIRs keep their stack
        // entry if present, else become untracked.
        if self.stack_pos.contains_key(&page) {
            self.status.insert(page, Status::HirNonRes);
        } else {
            self.status.remove(&page);
        }
        self.prune();
    }

    fn tracked(&self) -> usize {
        self.resident.len()
    }

    fn metadata_bytes(&self) -> u64 {
        ((self.status.len() + self.stack.len()) as u64).saturating_mul(24)
    }
}
