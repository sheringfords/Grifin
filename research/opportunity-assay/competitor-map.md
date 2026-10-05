# Competitor map (what exists, what it fails to show/do)

## Cost / FinOps

- AWS CloudWatch savings estimator + Cost Explorer + `StorageIOUsage`:
  proves totals (e.g. −28% example), manual spreadsheet, 5-min granularity,
  **no query attribution** ("CloudWatch Trap").
- CloudFix/Nops/Vantage: allocation-level advice (switch at I/O >25%,
  rightsizing, GP3) — **no IOPS→top-SQL join**; storage fix is guided rebuild.
- pganalyze/Datadog/Performance Insights: latency + waits, **not $**; no
  Standard-vs-I/O-Optimized breakeven per workload.
- Gap: nobody joins query → IOPS → $ → fix-list in one trusted artifact.

## Backup / restore

- `pgbackrest verify` / `pg_verifybackup`: checksum + manifest only — no
  mount, no WAL replay, no timeline cross, no version/role/extension parity,
  no RTO. Verify-green + restore-red is documented norm.
- Cloud snapshots: crash-consistent, no PG-level replay proof, no PITR-chain proof.
- Crunchy/CloudNativePG scheduled backups: schedule the backup, don't prove
  the restore. DataEgret timeline-verify preview: narrows one gap.
- Gap: boot-the-backup + replay + smoke-query proof with RTO numbers.

## Migration / DDL

- squawk (1.2k★, GH Action) / strong_migrations (4.5k★, Rails) /
  django-migration-linter / Atlas lint + pre-exec checks / Liquibase Policy
  Checks (Pro): **detection commoditized** — text-level rules.
- gh-ost / pt-osc (MySQL execution: won) · Vitess managed Online DDL /
  Cockroach online schema changes (platform execution: won) · pgroll
  (6.6k★, expand-contract via views: promising, view-indirection + DSL tax).
- Gap: size/load-aware risk (lock mode × table size × blocking PIDs),
  prod-clone dry-run verification, crash-safe backfill templates,
  multi-version app-compat enforcement. A gate that *simulates + rewrites*,
  not warns.

## LSM / compaction

- RocksDB built-ins: L0 triggers, pending-bytes stalls, rate limiter,
  Universal/Leveled/FIFO, BlobDB GC, subcompactions, Remote Compaction
  (experimental). Blunt per-CF thresholds; Universal SA unbounded; BlobDB
  GC unpredictable; ~50 interacting knobs.
- Pebble: debt knobs, excise, value separation, Cockroach admission control.
  #6200/#1329 show background bookkeeping and concurrency still bite.
- Managed: throttle + over-provision + off-peak manual compaction (cost,
  not fix).
- Research schedulers (SILK/ADOC/vLSM/DiaLSM/CaaS): ≥10 claimed 2–12× on
  RocksDB+YCSB, none upstreamed. Gap (narrow): offline stall explainer —
  which trigger fired, per-level SA/WA/RA, counterfactual option diffs.

## Observability

- pgbadger (offline slow-log) · pg_stat_statements + pg_wait_sampling +
  pg_stat_io (three separate rollups; sampling; superuser; no FS/block) ·
  PoWA/pganalyze joins · Percona PMM (engine cache pressure) ·
  Datadog DBM / New Relic (wait-group correlation, stops at DataFileRead
  label) · Coroot dual-layer (closest: eBPF wire + system views, still
  "correlated" by admission) · Parca/Pixie/groundcover (profiles/RED, no
  query↔bio join) · OTel collectors (10–60s poll).
- Gap claimed by vendors, refuted: true bio→query causality (async
  writeback + page cache + EBS + FTL opacity). Commoditized remainder:
  correlation dashboards.

## Physical design

- DTA / db2advis / Oracle Access Advisor (in-platform, won there) ·
  dexter+hypopg (2.1k★, linter-level) · pganalyze Index Advisor (upsell,
  48h processing, PG13.4 limits) · GCP index advisor (Enterprise Plus,
  CREATE-only) · Azure auto-indexing (only at-scale success — owns
  apply+validate+revert).
- Dead/assimilated standalone: OtterTune (dead 2024), EverSQL (→Aiven).
- Gap: none standalone; drop/consolidate-with-proof and
  partitioning/layout advising both require becoming an observability
  platform first.
