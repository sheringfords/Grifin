//! TinyLFU admission + S-LRU eviction (simplified W-TinyLFU).
//!
//! Mechanism (Einziger & Friedman, PDP '14 / arXiv:1512.00727 / ACM TOS
//! '17; W-TinyLFU window scheme from the Caffeine integration):
//! a compact frequency sketch decides admission by comparing the candidate's
//! estimated frequency against the eviction victim's. Newman pages go to a
//! probation (window-like) segment; re-referenced pages graduate to a
//! protected segment.
//!
//! Documented simplifications vs Caffeine's W-TinyLFU: single global sketch
//! (4-bit Count-Min, periodic halving) instead of TinyLFU+reset-on-size;
//! per-tier probation/protected segments instead of a global window+main
//! split; strict `>` admission comparison; no async maintenance. The core
//! ideas under test — frequency-based admission and scan resistance through
//! rejection of one-hit wonders — are preserved.

use crate::policy::{Decision, Policy, PolicyCtx, Tier};
use crate::trace::{PageId, TraceEvent};
use std::collections::{BTreeMap, HashMap};

const SKETCH_ROWS: usize = 4;
const SKETCH_WIDTH: usize = 2048;
const SKETCH_MAX: u8 = 15;
/// Halve all counters every this many increments (aging for phase changes).
const RESET_WINDOW: u64 = 16_384;

fn hash(row: usize, page: PageId) -> usize {
    // SplitMix64 with per-row seed; deterministic across platforms.
    let mut z = page
        .wrapping_add(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((row as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as usize % SKETCH_WIDTH
}

struct Sketch {
    table: [[u8; SKETCH_WIDTH]; SKETCH_ROWS],
    since_reset: u64,
}

impl Sketch {
    fn new() -> Self {
        Self {
            table: [[0; SKETCH_WIDTH]; SKETCH_ROWS],
            since_reset: 0,
        }
    }

    fn increment(&mut self, page: PageId) {
        for r in 0..SKETCH_ROWS {
            let i = hash(r, page);
            self.table[r][i] = self.table[r][i].saturating_add(1).min(SKETCH_MAX);
        }
        self.since_reset += 1;
        if self.since_reset >= RESET_WINDOW {
            self.since_reset = 0;
            for r in 0..SKETCH_ROWS {
                for v in self.table[r].iter_mut() {
                    *v >>= 1;
                }
            }
        }
    }

    fn estimate(&self, page: PageId) -> u8 {
        let mut m = SKETCH_MAX;
        for r in 0..SKETCH_ROWS {
            m = m.min(self.table[r][hash(r, page)]);
        }
        m
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Seg {
    Probation,
    Protected,
}

pub struct TinyLfu {
    ctx: PolicyCtx,
    sketch: Sketch,
    tick: u64,
    /// page -> (tier, seg, tick)
    pos: HashMap<PageId, (Tier, Seg, u64)>,
    /// (tier, seg) -> recency order. Index: tier*2 + seg.
    segs: [BTreeMap<u64, PageId>; 4],
}

impl TinyLfu {
    pub fn new(ctx: PolicyCtx) -> Self {
        Self {
            ctx,
            sketch: Sketch::new(),
            tick: 0,
            pos: HashMap::new(),
            segs: [
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            ],
        }
    }

    fn seg_idx(tier: Tier, seg: Seg) -> usize {
        tier.index() * 2
            + match seg {
                Seg::Probation => 0,
                Seg::Protected => 1,
            }
    }

    fn detach(&mut self, page: PageId) {
        if let Some((tier, seg, tick)) = self.pos.remove(&page) {
            self.segs[Self::seg_idx(tier, seg)].remove(&tick);
        }
    }

    fn attach(&mut self, page: PageId, tier: Tier, seg: Seg) {
        self.detach(page);
        self.tick += 1;
        self.segs[Self::seg_idx(tier, seg)].insert(self.tick, page);
        self.pos.insert(page, (tier, seg, self.tick));
    }

    fn combined_cap(&self) -> usize {
        self.ctx.t0_cap + self.ctx.t1_cap
    }

    fn protected_cap(&self, tier: Tier) -> usize {
        let cap = match tier {
            Tier::T0 => self.ctx.t0_cap,
            Tier::T1 => self.ctx.t1_cap,
        };
        (cap / 2).max(1)
    }

    fn protected_len(&self, tier: Tier) -> usize {
        self.segs[Self::seg_idx(tier, Seg::Protected)].len()
    }

    fn tail_of(&self, tier: Tier) -> Option<(PageId, Seg)> {
        for seg in [Seg::Probation, Seg::Protected] {
            if let Some((_, &page)) = self.segs[Self::seg_idx(tier, seg)].iter().next() {
                return Some((page, seg));
            }
        }
        None
    }
}

impl Policy for TinyLfu {
    fn name(&self) -> &str {
        "tinylfu"
    }
    fn config_str(&self) -> String {
        format!("tinylfu(sketch=4x{SKETCH_WIDTH}x4bit,reset={RESET_WINDOW},protected=50%)")
    }

    fn on_access(&mut self, ev: &TraceEvent, resident: Option<(Tier, bool)>) -> Decision {
        self.sketch.increment(ev.page_id);
        match resident {
            Some((tier, _)) => {
                let est = self.sketch.estimate(ev.page_id);
                let seg = self.pos.get(&ev.page_id).map(|(_, s, _)| *s);
                match seg {
                    Some(Seg::Probation) if est >= 2 => {
                        // Graduate to protected; make room if needed.
                        if self.protected_len(tier) >= self.protected_cap(tier) {
                            if let Some((_, &old)) =
                                self.segs[Self::seg_idx(tier, Seg::Protected)].iter().next()
                            {
                                self.attach(old, tier, Seg::Probation);
                            }
                        }
                        self.attach(ev.page_id, tier, Seg::Protected);
                    }
                    _ => {
                        self.attach(ev.page_id, tier, seg.unwrap_or(Seg::Probation));
                    }
                }
                Decision {
                    admit: None,
                    promote: tier == Tier::T1,
                }
            }
            None => {
                // Admission tier: probation tier T1 (T0 in single-tier mode).
                let tier = if self.ctx.t1_cap > 0 {
                    Tier::T1
                } else {
                    Tier::T0
                };
                let cap = match tier {
                    Tier::T0 => self.ctx.t0_cap,
                    Tier::T1 => self.ctx.t1_cap,
                };
                let occ = self.segs[Self::seg_idx(tier, Seg::Probation)].len()
                    + self.segs[Self::seg_idx(tier, Seg::Protected)].len();
                if occ < cap || self.pos.len() < self.combined_cap() {
                    // Room (or combined cache still filling): admit freely.
                    // The free-fill rule avoids a bootstrap deadlock where
                    // every candidate ties the incumbent at est==1.
                    Decision::admit(tier)
                } else if let Some((tail, _)) = self.tail_of(tier) {
                    // Exact TinyLFU admission test.
                    if self.sketch.estimate(ev.page_id) > self.sketch.estimate(tail) {
                        Decision::admit(tier)
                    } else {
                        Decision::bypass()
                    }
                } else {
                    Decision::bypass()
                }
            }
        }
    }

    fn victim(&mut self, tier: Tier) -> Option<PageId> {
        self.tail_of(tier).map(|(p, _)| p)
    }

    fn on_place(&mut self, page: PageId, tier: Tier) {
        // The admission test already ran in on_access (exact TinyLFU
        // compare-against-victim when the cache is full). New pages enter
        // the probation segment.
        self.attach(page, tier, Seg::Probation);
    }

    fn on_move(&mut self, page: PageId, _from: Tier, to: Tier) {
        // Demoted pages land in probation; promoted pages keep protected
        // status subject to the destination tier's protected cap.
        let want = match to {
            Tier::T0 => self
                .pos
                .get(&page)
                .map(|(_, s, _)| *s)
                .unwrap_or(Seg::Probation),
            Tier::T1 => Seg::Probation,
        };
        let seg = if want == Seg::Protected && self.protected_len(to) >= self.protected_cap(to) {
            Seg::Probation
        } else {
            want
        };
        self.attach(page, to, seg);
    }

    fn on_evict(&mut self, page: PageId, _tier: Tier) {
        self.detach(page);
    }

    fn tracked(&self) -> usize {
        self.pos.len()
    }

    fn metadata_bytes(&self) -> u64 {
        let sketch = (SKETCH_ROWS * SKETCH_WIDTH) as u64; // 4-bit packed as bytes
        sketch + (self.pos.len() as u64).saturating_mul(24)
    }
}
