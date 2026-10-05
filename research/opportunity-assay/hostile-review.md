# Hostile review (steelman against; no straw men)

## Against W1 (I/O-bill attribution)

1. AWS will absorb it: PI already has top-SQL; joining to billing is a small
   step. Counter: AWS profits from I/O opacity and sells I/O-Optimized as
   the answer; attribution that says "migrate off / fix queries" is
   directionally against ARPU. Weak counter — absorption still the top risk.
2. One good optimisation kills retention (success → churn). Counter: drift
   monitoring + CI gate convert one-shot savings into ongoing regression
   insurance; still, pricing must be value-share/upfront, not pure retainer.
3. Users don't care enough: many bills are small; pain concentrates in
   heavy tenants. Counter: targeting is explicit (high-I/O Standard
   clusters); failure criterion (<10% across 3 clusters) kills fast.
4. Datadog/pganalyze add a $ column. Counter: they sell latency/uptime, not
   FinOps; join quality + fix-list + breakeven math is the wedge, thin moat
   acknowledged (E3).

## Against W2 (restore proof)

1. Low-frequency pain won't sustain a product: teams test once, stop paying.
   Counter: compliance cadence (SOC2 backup-testing evidence) creates the
   repeat trigger — unvalidated, must be proven in interviews.
2. Scratch restores at TB scale cost hours/$100s per run — the cure's cost
   approaches the disease for the heaviest users. Counter: tiered
   cheap-daily/authoritative-weekly + delta restores; economics unproven.
3. Cloud vendors add "test restore" buttons (Crunchy/CNPG already schedule;
   proof is a small step). Counter: cross-timeline/version/extension parity
   + RTO measurement is the depth; still absorption-prone.
4. Selling fear fails without a recent outage. Acknowledged — event-driven
   GTM is the core commercial risk (I3).

## Against F (migration gate)

1. squawk + strong_migrations + Atlas already win CI; reviewers see "another
   linter" and stop reading. Counter: only a simulate+rewrite demo with
   missed-by-squawk cases earns attention — narrow path.
2. Size-aware analysis needs prod-shaped stats access that many CI pipelines
   lack; falls back to warn-only = duplicate. Acknowledged.
3. Backfill orchestration (the real unsolved core) is a much bigger build
   than a gate; the gate alone may not justify switching. Acknowledged —
   scope must stay gate-only until adoption is proven.

## Engineering-misleading risks (how v0 experiments could lie)

- W1: PI sampling + 5-min CloudWatch granularity misattributes bursty I/O;
  shared-buffer hits don't bill but look free (they are — fine); autovacuum
  background I/O attributed to "no query" bucket must be explicit, not hidden.
  Mitigation: report unattributed share loudly; kill if >50%.
- W2: ephemeral scratch restores on fast NVMe understate prod RTO on slow
  disks — report hardware-normalised RTO range, not a point number.
- F: dry-run on stale snapshot understates lock queues under concurrent load
  — gate must model queue amplification, not just lock mode.
- C: LOG-trace replay overfits one RocksDB version's picker — validate on ≥3
  traces across versions before claiming generality.

## Anti-novelty summary

W1: AWS estimator + CloudFix + PI eyeballing (allocation-level, no query
join). W2: `verify` + snapshots + runbooks (no proof). F: squawk/strong_
migrations/Atlas (detection, no simulation). C: ADOC/Pebble-#1329/Tuning
Guide (tune/schedule, none default; explainer refuses that game). None is
"ours uses AI" — all wedges are concrete joins/measurements incumbents
declined to build.
