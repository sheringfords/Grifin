# Grifin V1 — pre-registered experiment (frozen before final benchmarking)

Any edit to this file after the final matrix starts must be recorded below
with date, reason, and whether results were already seen. Thresholds must
never move after results are seen.

Amendment log: (none yet).

## Baselines (all against the same simulator contract)

Required: `lru` (per-tier), `clock` (per-tier), `arc` (FAST '03
transcription, two-tier adaptation documented in code), `tinylfu`
(admission + S-LRU, W-TinyLFU-simplified, documented).
Strong add-ons: `lirs` (SIGMETRICS '02 simplified, documented),
`static` (deliberately naive hot/cold split — the vacuity check).
Deferred with reason: CLOCK-Pro (≈ CLOCK + LIRS-history; both ingredients
present; beating both is stronger — see literature.md).

Candidate + ablations: `grifin-reuse`, `grifin-reuse-write`,
`grifin-reuse-relation`, `grifin-full` (config frozen in RESEARCH.md).

## Workloads (specs in `workloads/*.json`, deterministic by seed)

W1 stable hot-set (easy case) · W2 scan pollution · W3 hot-set shift ·
W4 write burst · W5 mixed relations · W6 short-lived churn ·
W7 analytical interference · W8 adversarial (designed to hurt Grifin:
misleading relation temps, write-bait, cache-sized cyclic stream).

## Protocol

- ~60,000 events/workload (scaled proportionally), seeds {1,2,3,4,5}.
- Warmup first 10% (state applies, metrics excluded).
- Report median ± 95% CI (normal approx; n=5, min/max shown alongside).
- Effect size: relative % delta vs best strong baseline (min-median-J among
  arc/tinylfu/lirs) per workload; no p-value theatre, no cherry-picking.
- Determinism: same trace+config+seed ⇒ identical simulated metrics
  (covered by `tests/policy_checks.rs`; wall-clock CPU excluded by nature).
- Sensitivity: re-rank cells under J coefficients ×0.25/×4; plus one
  halved-tier robustness run (T0=256/T1=1024) on the full matrix.
- Machine-readable: `results/<id>/{manifest,config,runs/*.json,
  summary.json,SUMMARY.md}`. Full runs are local artifacts; summary.json
  + manifest are committed.

## Success gate (frozen)

```text
KEEP iff ALL of:
  (a) >= 15% improvement in mean_lat OR p99 OR J for grifin-full vs the
      best strong baseline on >= 2 dynamic workloads (W2..W8), with
      non-overlapping-or-clearly-separated spreads (no overlapping-noise wins);
  (b) no > 5% regression of grifin-full vs best strong baseline on W1 on
      any primary measure;
  (c) policy overhead materially smaller than the benefit: grifin-full
      policy_ns/access < 10% of its mean-latency advantage, and J_total
      ranking agrees with J ranking on the winning workloads;
  (d) the advantage remains after coefficient sensitivity (×0.25/×4) and
      the halved-tier run;
  (e) an ablation attributes >= 5 points of the win to DB-semantic signals
      (full vs reuse-write gap), i.e. the win is not just clever replacement.
```

Outcomes:

- **KEEP** — gate passes; recommend one V2 direction.
- **WEAK_KEEP** — real but narrow signal (e.g. one workload, or J-only with
  latency tie, or sensitivity wobbles): mechanism interesting, evidence
  insufficient for product investment.
- **KILL** — gate fails: baselines match, wins only vs weak controls, no
  ablation delta, overhead dominates, or W8-style collapse is the norm.
  A negative result is a successful result; preserve everything.
- **INCONCLUSIVE** — evidence insufficient (noise dominates, simulator bug
  found late, workload misdesigned). Say so; do not round up.

## Kill conditions (explicit)

ARC/TinyLFU/LIRS match Grifin · wins only vs LRU/CLOCK/static · wins only
on workloads shaped like our heuristic · no ablation delta · overhead
dominates · benefits need unrealistic tier ratios · catastrophic failure
under common shifts · gain too small for integration complexity.
