//! Policy contract: every caching/placement policy implements this trait.
//!
//! The simulator owns all authoritative state (residency, dirty bits, tier
//! occupancy). Policies own only advisory metadata and answer two questions:
//! where should a missed page go (admission), and which page must leave a
//! full tier (victim). The simulator verifies every answer against ground
//! truth and panics on violation: a policy that returns a non-resident
//! victim fails loudly instead of silently corrupting the experiment.

use crate::trace::{PageId, TraceEvent};

/// Resident tiers. Absence from both tiers means the page lives on STORAGE.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Fast memory (DRAM-like).
    T0,
    /// Slower / tiered memory.
    T1,
}

impl Tier {
    pub fn index(self) -> usize {
        match self {
            Tier::T0 => 0,
            Tier::T1 => 1,
        }
    }
}

/// Capacities the policy must respect (in pages).
#[derive(Clone, Copy, Debug)]
pub struct PolicyCtx {
    pub t0_cap: usize,
    pub t1_cap: usize,
}

/// Placement intent returned by [`Policy::on_access`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Where to admit a missed page. `None` = bypass (leave on storage).
    /// Ignored for hits except as documented per policy.
    pub admit: Option<Tier>,
    /// For a hit in T1: request promotion to T0. Ignored otherwise.
    pub promote: bool,
}

impl Decision {
    pub fn bypass() -> Self {
        Self {
            admit: None,
            promote: false,
        }
    }
    pub fn admit(tier: Tier) -> Self {
        Self {
            admit: Some(tier),
            promote: false,
        }
    }
}

pub trait Policy {
    fn name(&self) -> &str;
    /// Machine-readable configuration for the reproducibility manifest.
    fn config_str(&self) -> String;

    /// Observe one access. `resident` is `Some((tier, dirty))` on a hit,
    /// `None` on a miss. Returns placement intent.
    ///
    /// Must be deterministic given the call history (no wall-clock, no
    /// thread-local randomness): same trace + config + seed => same result.
    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision;

    /// Choose a victim among pages currently resident in `tier`.
    /// Returning a non-resident page is a fatal simulator error.
    /// Returning `None` when the tier is non-empty is a fatal error.
    fn victim(&mut self, tier: Tier) -> Option<PageId>;

    // ---- placement notifications (actual outcomes, post-redirect) ----
    // The simulator calls exactly one of these per state change so the
    // policy's internal membership mirror cannot drift. `tracked()` lets
    // the simulator verify the mirror every event.

    /// `page` was admitted to `tier` (actual tier after zero-cap redirect).
    fn on_place(&mut self, page: PageId, tier: Tier);

    /// `page` moved between tiers (promotion or demotion). It stays resident.
    fn on_move(&mut self, page: PageId, from: Tier, to: Tier);

    /// Notification that `page` left `tier` for STORAGE (true eviction).
    fn on_evict(&mut self, page: PageId, tier: Tier);

    /// Number of pages the policy currently believes are resident.
    /// Must equal the simulator's resident count; asserted every event.
    fn tracked(&self) -> usize;

    /// Estimated steady-state metadata in bytes (sketches, tables, ghosts).
    /// Used to report metadata overhead; deliberately an estimate and
    /// labelled as such in results.
    fn metadata_bytes(&self) -> u64;
}
