# Grifin literature map

Status: foundation for V1. Evidence levels are tagged per item:

- **[P]** peer-reviewed conference/journal paper (venue independently verified
  during this mission via publisher/DBLP/USENIX/ACM pages).
- **[P2]** peer-reviewed per a secondary source (cited inside another paper or
  project page we verified); venue not independently opened.
- **[W]** workshop paper (peer-reviewed, lighter bar than conferences).
- **[E]** engineering material: project docs, specs, blogs, code. Useful for
  mechanisms, not citable as evidence.
- **[H]** our own hypothesis — must never be presented as literature.

Omitted rather than fabricated: AGE / D-FR (could not verify), SSD-iq
(could not verify), HIRE (could not verify). If they matter for V2, verify
first. This omission is deliberate: an unverifiable citation is worse than
a declared gap.

---

## 1. Cache / admission / replacement

### ARC — Adaptive Replacement Cache [P]

- paper: Megiddo & Modha, "ARC: A Self-Tuning, Low Overhead Replacement Cache"
- venue/year: USENIX FAST '03 (San Francisco).
- problem: LRU-family policies need workload-specific tuning (LRU-2, 2Q,
  LRFU, LIRS all carry parameters no single setting of which works
  everywhere); scans flush recency caches.
- baseline(s): LRU and fixed FRC_p policies; later follow-ups compare CAR.
- hardware: simulations + file-system traces (not hardware-sensitive).
- workload: 23 real-life traces (OLTP SPC-1-like synthetic, file system,
  web); cache sizes up to 4 GB.
- claimed result: ARC matches or beats the best *offline-tuned* fixed
  recency/frequency mix per workload (empirically universal); e.g. on an
  SPC-1-like trace at 4 GB, LRU 9.19% hit ratio vs ARC ~20%. O(1) per request.
- mechanism: two LRU lists (T1 recency, T2 frequency) + two ghost lists
  (B1, B2); single adaptation parameter `p` (target T1 size) moved on ghost
  hits. Scan resistance falls out: one-pass scans occupy T1 and are evicted
  from B1 without touching T2.
- assumptions: uniform object sizes; demand paging; hit ratio is the goal.
- limitations: hit ratio only (no write/migration/CPU accounting); single
  flat cache (no tiers); ghost directory doubles metadata.
- what Grifin should learn: ARC is the adaptivity bar. Any claim that
  "adapting to phases helps" must beat ARC on phase-change workloads, not
  just LRU. Our ARC transcription keeps list/ghost/p semantics exact and
  isolates the one forced deviation (two-tier victim restriction).

### LIRS — Low Inter-reference Recency Set [P]

- paper: Jiang & Zhang, "LIRS: An Efficient Low Inter-reference Recency Set
  Replacement Policy to Improve Buffer Cache Performance"
- venue/year: ACM SIGMETRICS '02 (extended version IEEE Trans. Computers '05).
- problem: recency predicts *next-reference time* badly for cold blocks with
  small recency (scans, loops); LRU keeps them while evicting hot blocks.
- baseline(s): LRU, 2Q, LRU-2, LRFU, ARC-era contemporaries.
- hardware: trace simulation.
- workload: production traces (DB2, OLTP, search engine); wide cache range.
- claimed result: large hit-ratio gains over LRU (often 2–10× miss
  reduction at small caches); competitive with or better than 2Q/LRU-2/LRFU
  with LRU-like overhead.
- mechanism: classify by Inter-Reference Recency (distinct blocks between
  two consecutive references — i.e. reuse distance). LIR set protected; HIR
  blocks replaceable; stack S + resident-HIR queue Q; bottom-of-stack HIR
  re-reference switches status with bottom LIR.
- assumptions: reuse distance estimable from history; single-level cache.
- limitations: intricate to implement correctly (status-switch corner
  cases); no notion of writes, tiers, or admission; bootstrap needs care
  (pure 1:1 switches cannot populate the LIR set).
- what Grifin should learn: reuse distance > raw recency is the reason LIRS
  beats LRU on scans/loops. Grifin's reuse term is a cheaper cousin of IRR;
  if Grifin wins anywhere LIRS also wins, the win is "clever replacement",
  not DB semantics — hence LIRS is a mandatory baseline and the ablation
  gate exists.

### CLOCK-Pro [P]

- paper: Jiang, Chen & Zhang, "CLOCK-Pro: An Effective Improvement of the
  CLOCK Replacement"
- venue/year: USENIX ATC '05. Linux 2.4.21 kernel implementation; follow-on
  CLOCK-Pro+ (SYSTOR '19) adds utility-driven adaptation.
- problem: CLOCK/ LRU cannot handle weak-locality (scan-like) accesses and
  pollute memory; LIRS is too costly for VM page replacement.
- baseline(s): CLOCK, CAR, LRU variants; kernel execution-time end-to-end.
- hardware: real Linux implementation + trace simulation.
- workload: VM + file I/O traces; reported up to 47% execution-time
  reduction on some programs vs CLOCK.
- claimed result: LIRS-like protection at CLOCK-like cost via three clock
  hands (hot/cold/test) and non-resident cold tracking.
- mechanism: cold pages get a "test period" (second chance via ghost
  history); hot/cold/test hands approximate LIRS's LIR/HIR split.
- assumptions: VM page granularity; cheap ref-bit scans acceptable.
- limitations: Linux never merged it (complexity vs benefit); evaluation is
  VM-centric, not DB buffer-pool centric.
- what Grifin should learn: CLOCK-Pro ≈ CLOCK + LIRS-history. V1 covers
  both ingredients separately (CLOCK baseline + LIRS baseline) and defers a
  full CLOCK-Pro transcription as low-marginal-information work. This is
  recorded as a scope decision, not an oversight: beating both ingredients
  is stronger than beating their hybrid, and losing to the hybrid later is
  a defined V2 falsifier.

### TinyLFU (+ W-TinyLFU) [P for the policy; E for Caffeine integration]

- paper: Einziger & Friedman, PDP '14 (short); full version Einziger,
  Friedman & Manes, arXiv:1512.00727 (2015), ACM Trans. Storage '17.
- problem: eviction policy matters less than *admission* under skew: most
  misses are one-hit wonders that should never displace incumbents.
- baseline(s): LRU, LFU, ARC, LIRS, random; synthetic Zipf + YouTube /
  Wikipedia / CDN traces.
- hardware: simulation (policy is hardware-agnostic).
- workload: skewed static distributions + real traces.
- claimed result: adding TinyLFU admission to *any* eviction policy lifts
  it near the best; W-TinyLFU (1% window LRU + SLRU main + TinyLFU
  admission) tops or equals every compared policy on all tested traces.
- mechanism: compact approximate-LFU sketch (counting Bloom / CM-sketch
  with periodic halving) estimates candidate vs victim frequency; admit iff
  candidate wins. W-TinyLFU adds a window to absorb recency bursts.
- assumptions: skew exists to exploit; sketch reset (aging) keeps up with
  phase changes — the paper's weak spot under fast churn.
- limitations: frequency-only (write-blind, tier-blind); reset-window
  tuning decides phase agility; ties/bursts need the window.
- what Grifin should learn: (1) admission >> eviction under skew — Grifin
  is designed admission-first with bypass; (2) our transcription preserves
  the exact admit-iff-candidate-beats-victim test; (3) TinyLFU's churn
  collapse (stale sketch rejects the new hot set) is a *predicted* Grifin
  opportunity (decay + relation signals) and a falsifier if Grifin shows
  the same collapse.

### LeCaR [W] / CACHEUS [P2]

- paper: Vietri et al., "Driving Cache Replacement with ML-based LeCaR",
  HotStorage '18 (workshop). Follow-up CACHEUS (Rodriguez et al., FAST '21
  per secondary citation — venue not independently opened).
- problem: can online learning (regret minimisation over experts: LRU +
  LFU) beat hand-tuned adaptive policies?
- baseline(s): ARC, LRU, LFU; FIU production traces + synthetic phases.
- workload: small caches relative to working set (0.1–1%).
- claimed result: LeCaR beats ARC by up to 18× hit *rate ratio* at 0.1%
  cache sizes (note: ratio on tiny denominators), competitive at large
  caches. Later reproductions show smaller/negative gaps — treat the 18×
  as a small-cache artifact, not a general claim.
- mechanism: two experts (recency/frequency), exponential-weights over
  regret on eviction mistakes. CACHEUS adds scan/churn experts.
- assumptions: expert set covers workload modes; learning rate tuned.
- limitations: workshop-level evaluation; ML-in-the-loop cost rarely
  accounted per decision; gains concentrate where any adaptive policy wins.
- what Grifin should learn: V1 uses NO learning (mission constraint) — but
  LeCaR/CACHEUS define the "adaptive enough?" bar for V2. If V0's fixed
  weights + decay match LeCaR-style adaptivity on our phases, learning is
  unnecessary complexity; if V0 loses where LeCaR wins, that is a precise,
  publishable negative result about fixed-weight value models.

### S3-FIFO and the "simple beats clever" line [H, unverified]

- Not reviewed to evidence standard in V1 (venue/year not verified during
  this mission). Hypothesis only: recent FIFO-with-ghosts work suggests
  much of ARC/LIRS complexity buys little on modern traces. V1's `static`
  control and the LRU-vs-ARC gaps (or lack thereof) in our results are the
  empirical substitute. Verify before V2.

---

## 2. Database buffer management

### LeanStore [P] (+ VLDB '24 retrospective [P])

- paper: Leis, Haubenschild, Kemper & Neumann, "LeanStore: In-Memory Data
  Management beyond Main Memory", ICDE '18. Retrospective: Leis,
  "LeanStore: A High-Performance Storage Engine for NVMe SSDs", PVLDB
  17(12), 2024.
- problem: traditional buffer managers (Shore-MT lineage) burn ~all
  instruction budget on fix/unfix, latching, and replacement overhead;
  NVMe arrays sit idle.
- baseline(s): in-memory B-tree, BerkeleyDB, WiredTiger; TPC-C.
- hardware: many-core + NVMe (direct-attached arrays in later work).
- workload: TPC-C (100 warehouses), YCSB frontends.
- claimed result: ~67K vs 69K tps single-thread vs a pure in-memory B-tree
  (ICDE '18); full NVMe exploitation in later branches.
- mechanism: pointer swizzling (zero-cost hot access), optimistic lock
  coupling, lightweight two-stage replacement (random + cooling FIFO =
  LeanEvict), distributed logging, out-of-place SSD writes ("How to Write
  to SSDs", VLDB '26, Lee/Ziegler/Leis [P2-forthcoming]).
- assumptions: 4 KiB pages; OLTP-friendly; SSD bandwidth is the ceiling.
- limitations: replacement is deliberately *dumb* (LeanEvict) — the
  project's bet is that replacement barely matters when misses are cheap
  and hot access is free. This is the strongest prior against Grifin: if
  LeanEvict suffices on NVMe, placement cleverness is wasted.
- what Grifin should learn: (1) hot-path CPU is sacred — Grifin V0 budgets
  O(1)-ish decisions (sampling victim, lazy decay); (2) our simulator must
  one day be validated against LeanStore traces, not just synthetic Zipf;
  (3) out-of-place writes bound the write-amplification term of J.

### WATT — Write-Aware Timestamp Tracking [P]

- paper: Vöhringer & Leis, "Write-Aware Timestamp Tracking", VLDB '23.
- problem: classical replacement is write-blind; dirty evictions cost
  real SSD writes, and write-heavy skewed workloads punish LRU/LeanEvict.
- baseline(s): ten algorithms in their simulation framework + LeanEvict,
  Random, Hyperbolic in end-to-end LeanStore.
- hardware: NVMe SSD servers; simulation + integrated engine.
- workload: five traces incl. write-heavy TPC-C variants.
- claimed result: WATT improves out-of-memory performance substantially
  without hurting in-memory performance; needs fewest reads/txn via
  sub-frequency tracking.
- mechanism: per-page value function with write/recency components +
  *sampling-based* victim selection (evaluate a sample, evict min). Direct
  ancestor of Grifin V0's design (value + sampling).
- assumptions: page-write cost >> bookkeeping cost; sampling is
  representative.
- limitations: no cross-object semantics (page-local only); single tier.
- what Grifin should learn: **WATT is the closest prior work and the top
  novelty risk.** Grifin V0's write term + sampling is WATT-like; the
  claimed delta is strictly the DB-semantic layer (relation temperature,
  page-type/scan-at-object-level, lifetime). The ablation gate
  (grifin-reuse-write vs grifin-full) exists precisely to test whether that
  layer adds anything over a WATT-like baseline. If it does not, outcome is
  KILL-with-credit-to-WATT.

### Umbra [P]

- paper: Neumann & Freitag, "Umbra: A Disk-Based System with In-Memory
  Performance", CIDR '20.
- problem: in-memory systems (HyPer) die past DRAM; disk systems tax the
  cached working set.
- baseline(s): HyPer (in-memory), disk-based competitors; analytical +
  transactional workloads.
- claimed result: in-memory performance while cached, graceful degradation
  beyond; variable-size pages, low-overhead buffer manager.
- mechanism: memory-optimised buffer manager (page fixation without
  traditional fix/unfix overhead), variable-size pages.
- assumptions: SSDs fast enough that miss penalty is tolerable if rare.
- limitations: replacement policy is not the paper's focus — again the
  "replacement barely matters" prior.
- what Grifin should learn: the natural V2 host for trace collection and
  buffer-pool integration (more accessible than PostgreSQL internals, real
  engine, TUM lineage shared with LeanStore).

### vmcache / Virtual-Memory Assisted Buffer Management [P2/E]

- Mechanism known from the LeanStore project (vmcache branch/design docs):
  use MMU/VM tricks (mmap, page faults, MADV) to manage residency with
  near-zero software bookkeeping. Related: "Predictive Translation"
  (Zinsmeister/Nguyen/Leis/Neumann, SIGMOD '26 per secondary source —
  forthcoming, do not cite as established).
- what Grifin should learn: the endgame of "policy overhead must be ~zero"
  may be hardware-assisted residency, not smarter software policy. V1's
  measured ns/access overhead column is the bridge to that discussion.

### Tiered-memory / CXL buffer management [H, gap]

- No verified canonical paper reviewed in V1. CXL/memory-tiering changes
  the latency ratios our T0/T1 model stands in for; V1 keeps tiers as
  parameters + sensitivity analysis rather than CXL claims. Declared gap
  for V2 background work.

---

## 3. Storage / SSD co-design

### What Modern NVMe Storage Can Do (Haas & Leis, VLDB '23) [P]

- Verified via project bibliography (PVLDB citation). Establishes NVMe
  QD/parallelism behaviour that justifies our storage latency parameters
  as *parameters* and motivates io_uring-style V2 work. Lesson: model
  bandwidth + latency, never latency alone (our tier spec carries both
  fields; V1 exercises latency first).

### How to Write to SSDs (Lee, Ziegler & Leis, VLDB '26) [P2-forthcoming]

- Out-of-place SSD write behaviour from a DB perspective. Do not cite as
  established; listed so V2 knows where the write-amplification prior
  comes from.

### FDP / ZNS from a database perspective [E]

- NVMe FDP and ZNS specifications are engineering documents: placement
  directives (FDP) and sequential-write zones (ZNS) move placement control
  toward the host — philosophically aligned with Grifin (host *knows*
  semantics), but V1 implements neither. No DB-specific performance claims
  taken from this material.

---

## 4. Logging / NVMe commit path

### Moving on From Group Commit (Nguyen et al., SIGMOD '25) [P2]

- Autonomous commit on NVMe questions the universality of group commit.
  Lesson for Grifin: "conventional wisdom calibrated on slow storage"
  fails on NVMe — our tier latencies must be re-examined per hardware
  generation, and V1 results are conditional on the parameter set
  (recorded in every manifest).

### Rethinking Logging/Recovery (Haubenschild et al., SIGMOD '20) [P2]

- Distributed per-thread logging, fuzzy checkpoints. Background for V2
  engine integration; not directly used in V1.

---

## 5. Index / storage architecture (attribution caution)

### Learned indexes (Kraska et al., SIGMOD '18) [P]

- "The Case for Learned Index Structures" + follow-ups (instance-optimised
  systems, VLDB). Key lesson for Grifin: on disk-based systems the learned
  advantage often comes from *smaller index size saving an I/O*, not faster
  lookup. Generalised caution: **never attribute to placement what belongs
  to layout/indexing.** V1 holds layout fixed (same trace, same pages for
  every policy) so no policy can win via layout; the workload suite varies
  access pattern, never structure.

---

## 6. Novelty-risk assessment (pre-implementation, frozen)

Closest prior work, in order:

1. **WATT (VLDB '23)** — value function + sampling + write-awareness.
   Grifin V0 without the relation/lifetime layer is arguably WATT-like;
   the ablations isolate exactly this.
2. **TinyLFU-admission + SLRU (TOS '17)** — our strongest baseline; Grifin
   must win on *dynamics* (phases, churn, mixed relations), since TinyLFU
   owns static skew.
3. **ARC (FAST '03)** — the adaptivity bar on scans/shifts.
4. **LeanEvict (ICDE '18)** — the "dumb is enough" null hypothesis,
   approximated by our LRU/CLOCK controls (LeanEvict itself is
   random+cooling-FIFO; we do not claim to transcribe it).

Grifin V0's claimed delta: relation-level temperature + object-level scan
detection + lifetime devaluation + write/endurance-aware value, as
*ablatable additive terms* over a WATT-like core. If the full-minus-parts
ablation is ~zero, there is no novelty and the project outcome is KILL
with a pointer to WATT/TinyLFU. If relation identity alone carries the
win, that is a narrow, publishable, product-relevant mechanism (relation
tags already exist in every engine's buffer pool).
