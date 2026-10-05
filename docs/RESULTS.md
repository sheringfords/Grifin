# Grifin V1 — results and decision (post-registration; gate frozen in EXPERIMENT.md)

## Decision

```text
KILL
```

The DB-semantic layer of Grifin V0 adds churn, not information. The
WATT-like core (`grifin-reuse-write`) is competitive-but-unremarkable; the
relation/lifetime additions regress it nearly everywhere. H0 stands.

## Gate checklist (from frozen EXPERIMENT.md)

- (a) ≥15% win on ≥2 dynamic workloads: **FAIL.** Best cells: W2 −4.6%
  (mean and J, clearly separated), W7 p99 −24.2% with mean **+92.6%**
  (not a win). Nothing else breaks −5%.
- (b) W1 regression ≤5%: **FAIL.** grifin-full W1: mean +12.9%, J +15.5%
  vs best strong baseline (lirs). (Core `reuse-write` passes at +2.9% —
  see diagnosis.)
- (c) overhead « benefit: moot — no benefit. (Grifin CPU 0.3–3us/access,
  comparable to baselines; J excludes CPU by design.)
- (d) sensitivity + halved tiers: **FAIL.** Zero WIN cells for grifin-full
  under any J variant (9 combos); halved-tier run loses by *more*
  (+11%…+94%).
- (e) ablation attributes ≥5 pts to DB semantics: **FAIL, inverted.** The
  semantic layer *costs*: full is worse than reuse-write on 5/8 workloads
  (better only on W2 −12.6%, W7 −14.8%, W8 −0.7% internally — yet still
  losing to baselines on all three).

## Head-to-head (grifin-full vs best of {arc,tinylfu,lirs}; medians, n=5)

Quoted from `experiments/v1-final/SUMMARY.md` (generated artifact).

| workload | mean_lat Δ | J Δ | note |
|---|---|---|---|
| w1 stable | **+12.9%** | **+15.5%** | regression on the easy case |
| w2 scan | −4.6% | −4.6% | only real win; small |
| w3 shift | **+20.5%** | **+21.7%** | stale relation-temp damage |
| w4 burst | +5.6% | +6.8% | write-awareness doesn't pay |
| w5 mixed | +12.2% | +12.0% | relation identity misleads |
| w6 churn | **+42.9%** | **+42.7%** | semantic bonus saturates → churn |
| w7 analytic | **+92.6%** | +90.7% | scan-bypass vs *repeated* scans |
| w8 adversarial | +2.6% | +2.6% | trap works as designed |

Spreads are tiny (CI half-widths ≪ deltas); these are systematic, not noise.

## Workloads

- Grifin wins: **none** at gate magnitude. Micro-win: W2 (−4.6%, separated).
- Ties: W8 (+2.6%, within noise band); W4 (+5.6%, marginal loss).
- Loses: W1, W3, W5, W6, W7.
- Worst regression: W7 mean +92.6% (hit 56.9% vs tinylfu 80.3%).

## Diagnosis (mechanism, not excuses)

1. **Value saturation → T0-everything → demote churn.** Decayed counts
   saturate over 60k events (epoch halving capped, fixed thresholds), so the
   relation-temperature bonus pushes nearly everything over TH_T0. W6:
   full migrates 19,938× vs reuse 96×. The policy drowns in its own
   promotions. Scale-dependent: at 20k-event previews this was invisible —
   which vindicates the 5-seed/full-length protocol and condemns fixed
   weights + weak decay.
2. **Scan-bypass misfires on repeated scans.** W7's scans repeat the same
   region (generator fact, disclosed below); bypassing them forfeits
   23 points of hit ratio. "Scan ⇒ cold" is false for periodic analytics.
   Genuine write win inside the loss: phys_writes = 0 — but latency cost
   dominates.
3. **Relation temperature is stale information under shifts.** W3: the old
   hot relation's bonus survives the shift (decay too slow) while ARC's
   ghosts adapt in one pass (+20.5%).
4. **Write-awareness doesn't pay at these ratios.** W4: dirty-bonus
   retention saves nothing measurable; burst writes are compulsory misses
   for everyone.

## What the baselines did (simulator sanity)

Controls behaved per literature, so the instrument is trusted: ARC most
scan-resistant on W2; TinyLFU collapses on churn (W6 hit 55.7%, stale
sketch — predicted in literature.md); LIRS best baseline on 6/8 (the
simplified transcription is genuinely strong — the policy to beat);
`static` fails everywhere (vacuity check passes). LIRS migration counts
near zero on W5/W6: calm promotion discipline is the actual lesson.

## Caveats / threats

- Post-hoc ARC sensitivity (independent-audit suggestion, run after the
  gated matrix, code reverted afterwards): a textbook p-conditional victim
  (prefer T1 tail iff |T1| ≥ max(p,1)) improves frozen ARC by ~30% on W6
  churn (35755 → ~24324 mean, hit 65.6% → 80.4%) and ~nothing elsewhere.
  The frozen transcription was thus handicapped on churn — and KILL still
  stands (best baseline on W6 remains LIRS 22798; no gate clause moves).
  Lesson: our ARC deviation protected fresh one-touch pages at the expense
  of hot ones under turnover; textbook REPLACE does the opposite.
- Synthetic traces only; no production validation (declared V1 scope).
- My LIRS `lir_count()` is O(n) per HIR hit — its CPU column is inflated
  (J excludes CPU, so rankings stand; overhead table read with care).
- W7 scans repeat the identical region each period (implementation fact);
  result reads as "repeated-scan handling", which is legitimate but narrower
  than "analytical interference" in general.
- Single-threaded sim; no bandwidth contention modelling exercised.
- Early previews (1 seed, 20k events) looked competitive; full protocol
  reversed them. One-run benchmarks prove nothing — this mission's central
  methodological lesson.

## Artefacts

- `experiments/v1-final/{SUMMARY.md,summary.json,manifest.json}` (+
  `v1-tiers-half`, + `sensitivity-*.txt`). Full runs in local `results/`.
- All tables above are quoted from generated artifacts, not hand-copied.

## Next experiment (if ever — KILL means no product investment)

A V2 would need: (i) adaptive thresholds/stronger aging (fixed weights are
dead); (ii) repeat-aware scan handling (periodicity detection, not blind
bypass); (iii) validation against LeanStore/Umbra traces before any engine
integration. None of this is authorised by this result; it is recorded so
the failure teaches.
