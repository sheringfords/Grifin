# Grifin V1 — research statement (frozen)

## Research question (narrow; do not broaden)

> Can database-semantic information improve page admission, retention, and
> placement decisions enough to beat strong generic caching policies after
> accounting for policy overhead, migrations, write traffic, and changing
> workloads?

Out of scope for V1: making databases faster in general; indexing/layout;
CXL/ZNS/FDP/kernel/PostgreSQL integration; any ML in the policy.

## Hypothesis H1

Pages carry engine-visible semantics — relation identity, page type, dirty
state, update frequency, expected lifetime — that generic recency/frequency
history does not capture. A placement policy that prices these signals into
an explicit per-page value function will lower total cost (modelled latency
+ endurance-relevant writes + migration churn) on dynamic workloads by
enough to matter (>15% on a primary measure), without regressing stable
workloads (>5%), once policy CPU and metadata are counted.

Null H0: ARC/TinyLFU/LIRS-class generic policies match Grifin everywhere
that matters; DB semantics add no measurable information.

## Formal model

Access to page `p` at logical time `t` (event index):

```text
TraceEvent { seq=t, page_id=p, relation_id, op: READ|WRITE, page_type,
             txn_id, size_bytes, relation_kind?, logical_group?,
             creation_time? }
```

Optional fields are genuinely optional. No policy may require them.

Tiers (simulation parameters, not hardware claims):

```text
Tier 0 FAST_MEMORY:  cap 512 pages,  read 80ns,   write 120ns
Tier 1 SLOW_MEMORY:  cap 2048 pages, read 400ns,  write 600ns
STORAGE (NVMe-like): unbounded,      read 70us,   write 90us
migration T0<->T1: 1000ns. page 8KiB. warmup: first 10% of events.
```

On T0 pressure the simulator demotes victims to T1 when T1 has room
(migration cost), else evicts to storage (writeback if dirty). Bypassed
writes go through to storage. All policies face the identical simulator.

## Objective

Primary composite (frozen coefficients — no tuning after this point):

```text
J = mean_lat_ns + 4000 * (physical_writes / accesses)
                  + 200 * (migrations / accesses)
```

The extra weights are deliberately SMALL vs modelled media latencies
(90us/write, 1000ns/migration): J ≈ latency + modest endurance/churn
penalty. Policy wall-clock CPU is reported separately (ns/access) and gated
by the overhead check, not folded into J (so J cannot hinge on whose code
was micro-optimised). `J_total = J + policy_ns/access` is reported for the
system-level view. Raw metrics always accompany J; sensitivity re-ranks
under write_w/migr_w ×0.25/×4.

Primary measures: `mean_lat_ns`, `p99_lat_ns`, `J`.
Reported always: hits per tier, phys reads/writes, bytes written,
migrations/promotions/demotions, evictions, admissions/rejections,
occupancy, policy ns/access, metadata bytes/page.

## Candidate mechanism (Grifin V0, frozen config)

```text
value(p) = 1.0 * ln(1+freq) + 0.8 * write_frac + 0.5 * dirty
         + 0.6 * ln(1+rel_freq)/4 - 3.0 * scan_mode - 1.5 * one_hit_cold
admit: value >= 2.4 -> T0; >= 0.2 -> T1; else bypass.
victim: min value among 8 sampled residents (LeanEvict/WATT precedent).
promote T1->T0 when value >= 2.4. decay: epoch halving every 4096 events.
scan: stride-1 run >= 24 within a relation. lifetime: single-touch pages
      younger than 512 events devalued unless relation hot.
```

Calibration history (pre-freeze exploration, seed 1, 20k events, W1+W2):
TH_T0 swept {1.2, 1.8, 2.4}; 2.4 minimised stable-workload churn without
harming scan behaviour. Frozen at 2.4 before the final matrix. All other
constants are literature-motivated round numbers, never swept.

Ablations (all frozen, all run): `grifin-reuse` (freq only),
`grifIn-reuse-write` (+write/dirty), `grifin-reuse-relation` (+relation
temperature + scan detection), `grifin-full` (+lifetime).

## What would change our minds

- Strong baselines tie/beat Grifin on W2–W8 → H0 stands (KILL).
- Wins only vs LRU/CLOCK/static → H0 stands (KILL).
- Full ≈ reuse-write (no ablation delta) → the DB-semantic layer adds
  nothing; mechanism is "WATT-like core", not Grifin (KILL with credit).
- Wins vanish when CPU/metadata counted, or under coefficient sensitivity,
  or under halved tier sizes → INCONCLUSIVE at best.
