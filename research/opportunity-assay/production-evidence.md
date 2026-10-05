# Production evidence (consequences disclosed where available)

## Cloud DB cost pain (recurring $, one-way ratchets)

- HN: fuzzy queries → $3,000/mo for a $50–100 DB, 15M rows [A] —
  https://news.ycombinator.com/item?id=45930316
- r/aws: Aurora IOPS alone modeled ~$13,400/mo, total ~$18,432/mo [A] —
  https://www.reddit.com/r/aws/comments/1bgv990/question_on_provisioning_aurora_postgres
- re:Post: 22.4B I/Os → $4,483.58 reconciled via CloudWatch RUNNING_SUM [E] —
  https://repost.aws/articles/AR7zWYUJDIRUWYHsmcouQdiw/how-to-calculate-iops-usage-for-your-aurora-db-clusters
- "CloudWatch Trap": I/O-Optimized blind, reverted to Standard, saved 26% [E] —
  https://dev.to/aws-builders/aurora-serverless-v2-when-io-optimized-actually-costs-you-more-4bb2
- RDS autoscaling "saved us from outage, cost us $12k"; allocated 3000GB,
  using 1300GB, cannot shrink in place [P/A/E] —
  https://medium.com/engineering-playbook/rds-storage-autoscaling-saved-us-from-outage-cost-us-12k-140a4a202072 ·
  https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/USER_PIOPS.Autoscaling.html
- Vendor syntheses: $0.20/M I/Os "silent cost bomb", background (incl.
  autovacuum) I/O billed, breakeven ~25% of bill / ~4,000 IOPS [W] —
  https://planetscale.com/blog/amazon-aurora-pricing-the-many-surprising-costs-of-running-an-aurora-database

## Restore failures (outage hours + data-loss hours)

- GitLab 2017 [P]: 24h outage, ~6h data loss; pg_dump silently failing for
  weeks (9.2 vs 9.6 mismatch); snapshots not enabled —
  https://about.gitlab.com/blog/postmortem-of-database-outage-of-january-31
- Matrix.org Sep 2025 [P]: 24h outage; 55TB restore >10h + 17h replay;
  outdated wal-g broke incremental restore —
  https://matrix.org/blog/2025/10/post-mortem
- Matrix.org Jul 2025 [P2]: corruption undetected Jan-2021→Jul-2025; test
  restore cost "hundreds of USD/day" —
  https://matrix.org/blog/2025/07/postgres-corruption-postmortem
- pgBackRest gaps that pass `verify` [E]: 11h WAL gap despite archive-push
  success (#1616); `.partial` WAL breaks PITR (#1952); timeline-fork errors —
  https://github.com/pgbackrest/pgbackrest/issues/1616 ·
  https://github.com/pgbackrest/pgbackrest/issues/1952
- "Nobody tests restores" genre [A multiples] —
  https://www.reddit.com/r/devops/comments/1v0m5nh/does_anyone_actually_test_their_database_restores

## Migration incidents (same 5 patterns, ~15 years)

- Autumn 2026: collation migration lock conflict, 75min, 10% API fail,
  rollback deadlocked —
  https://useautumn.com/blog/post-mortem-database-outage-caused-by-collation-migration-locking-conflict.md
- RevenueCat Aurora PG10→14: 5h outage 2022 —
  https://www.revenuecat.com/blog/engineering/postmortem-aurora-postgres-migration
- Heroku `ALTER TYPE`: 25min total block; "400ms migration → 20min cascade"
  (analytics SELECT queued behind ACCESS EXCLUSIVE, pool exhausted) —
  https://gist.github.com/dwbutler/1034446c1aba231ca8d8639d3be78c6b
- SET NOT NULL on 1B rows: 20min ACCESS EXCLUSIVE; 15-footgun catalogs exist —
  https://dev.to/isabelle_hue/the-15-postgres-migration-footguns-that-lock-production-and-how-to-catch-them-in-pr-review-4dap
- Stable pattern: (1) CREATE INDEX w/o CONCURRENTLY (2) NOT NULL/CHECK scan
  (3) ALTER TYPE rewrite (4) ADD COLUMN+DEFAULT+NOT NULL+backfill in one txn
  (5) lock-queue amplification + CONCURRENTLY-in-txn failure.

## Compaction stalls (seconds, continuous)

- RocksDB #10903 (open): intra-L0 starvation → ~50s write stall —
  https://github.com/facebook/rocksdb/issues/10903
- RocksDB #9561 (open): Universal SA unbounded (Callaghan: leveled 2.0×,
  universal 5.6×, blob-tuned 23.2×; IO-bound universal OOM-full ~6×) —
  https://github.com/facebook/rocksdb/issues/9561
- Pebble #6200 (24.3–26.1): delete-hint rescan under DB.mu freezes all
  writes incl. Raft appends for seconds —
  https://github.com/cockroachdb/pebble/issues/6200
- Pebble #1329: compaction concurrency still manual (NVMe vs read bursts) —
  https://github.com/cockroachdb/pebble/issues/1329
- 166 open/closed "stall" hits in facebook/rocksdb; 70 TiKV write-stall hits.

## Pattern assessment

Cost pain: high-frequency, $-quantified, permanent ratchets. Restore pain:
low-frequency, catastrophic, "should have proven restores" endings.
Migration pain: high-frequency, same patterns despite linters (detection
solved, execution/compat not). Compaction pain: high-frequency in LSM
shops, structurally mitigated never solved.
