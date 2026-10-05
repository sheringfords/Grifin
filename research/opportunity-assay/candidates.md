# Candidates (full writeups; shortlist in recommendation.md)

## W1 — Cloud-DB I/O-bill attribution (query → IOPS → $ → fix-list)

- Problem: Aurora/RDS I/O bills surprise ($3k/mo for a $50 DB; $13k/mo IOPS
  modeled); one-way storage ratchet; Standard-vs-I/O-Optimized blindness.
- User: backend eng + SRE at Aurora/RDS Postgres shops; buyer: eng
  leadership/FinOps (recurring $ line item). Trigger: bill shock or deploy
  that moves IOPS. Frequency: monthly + every deploy.
- Current workaround: AWS estimator spreadsheet + PI eyeballing + CloudFix
  allocation advice. Fails at: which query burned the money.
- Wedge: read-only CLI + weekly cron joining PI top-SQL +
  pg_stat_statements + CloudWatch IOPS + Cost Explorer → top-10 queries by
  $/mo, breakeven math, top-5 fixes; later CI gate on EXPLAIN (BUFFERS).
- Deployment: read-only IAM + read-only PG user. No restart/proxy/agent.
  Friction: PI must be enabled; pricing drift handled in docs.
- Commercial: FinOps ROI (save-share or seat); churn-after-fix risk,
  mitigated by drift monitoring + CI gate (ongoing value).
- Failure criterion: <10% attributable savings across 3 clusters → abandon.

## W2 — Restore proof + RTO measurement (boot the backup)

- Problem: verify-green/restore-red (GitLab 2017, Matrix 2025: 24h outages,
  55TB/>10h restores, 11h WAL gaps, timeline forks, version skew).
- User: SRE/platform; buyer: eng leadership post-trauma or for SOC2 backup-
  testing evidence. Trigger: outage, audit, TB-scale growth. Frequency: low
  per team (the crux).
- Current workaround: `verify`, snapshots, "test restores regularly"
  (unfunded at TB scale/$100s-per-day scratch).
- Wedge: off-prod CLI — restore to ephemeral volume, WAL replay to latest,
  read-txn + smoke queries + amcheck spot-checks → PASS/FAIL + WAL-gap +
  RTO + GB/hr. Cheap daily gap-check + weekly full proof.
- Deployment: MEDIUM — repo/S3 read creds + scratch compute; no prod impact.
- Commercial: insurance sale; compliance (SOC2) is the repeatable trigger.
- Failure criterion: 30 days with manual RTO < SLO and gaps never firing on
  version-pinned single-timeline repos → abandon. Validate WTP first:
  5 interviews + 1 pilot before code.

## F — Migration gate that simulates + rewrites (Postgres)

- Problem: same 5 lock/scan/backfill patterns for 15 years; staging ≠ prod.
- User: backend devs shipping DDL; buyer: platform teams. Frequency: every
  deploy with a migration.
- Current workaround: squawk/strong_migrations/Atlas (text lint — won).
- Wedge (narrow): parse DDL → version-aware lock catalog → size-aware
  duration from pg_class/stats → FAIL with lock mode + estimate + safe
  2–3-step rewrite + timeout/retry wrapper + backfill template, optionally
  verified on ephemeral prod-clone. Must demo missed-by-squawk value.
- Deployment: GH Action + optional read-only DSN. Friction low for lint,
  higher for dry-run.
- Failure criterion: cannot beat squawk on a labeled incident corpus, or
  teams won't adopt rewrites over warnings → abandon. RESEARCH first.

## C — LSM stall explainer (offline, refuse to auto-tune)

- Problem: 10–50s write stalls, SA 2–6×, GC unpredictability (RocksDB
  #10903/#9561, Pebble #6200, 166 stall threads).
- User: storage/infra eng at LSM-heavy shops. Frequency: high there.
- Current workaround: blunt built-ins + over-provision + off-peak manual
  compaction. Research schedulers (≥10) never upstreamed.
- Wedge (sliver): offline `LOG`+OPTIONS+metrics analyzer — which trigger
  fired, per-level SA/WA/RA, counterfactual option diffs. NO control loop.
- Deployment: offline CLI, zero prod touch.
- Failure criterion: cannot attribute documented stalls on 3 public traces,
  or target users are only hyperscalers who build internally → abandon.
  RESEARCH first (2 interviews + 3 LOG traces, no code).

## Dropped (see hostile-review.md for kill details)

- A (WA attribution): coarse attribution solved; precise attribution
  impossible where pain lives (cloud). DROP.
- B (SSD-aware writes): gains inside engines/vendors; FDP non-portable;
  no standalone insertion point. DROP.
- D (storage-path profiler): causality unobservable (async writeback, page
  cache, EBS, FTL). Correlatable remainder commoditized. DROP.
- E (design advisor): 30-yr autopsy — monetises only inside platforms with
  telemetry + auto-revert. DROP standalone.
