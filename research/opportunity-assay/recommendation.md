# Recommendation (shortlist ≤5; at most one BUILD)

## Candidate: W1 — Cloud-DB I/O-bill attribution
Decision: BUILD

Problem: Aurora/RDS I/O bills surprise and ratchet; nobody attributes
billed IOPS to queries.
User: backend eng/SRE + FinOps buyer at Postgres-on-Aurora/RDS shops.
Existing pain evidence: $3k/mo surprises, $13–18k/mo modeled bills, 26%
saved by tier switch, unshrinkable storage (production-evidence.md).
Why existing solutions are insufficient: estimator proves totals;
CloudFix/Vantage advise allocation; monitors show latency — none joins
query → IOPS → $ → fix-list.
Technical wedge: read-only join (PI top-SQL + pg_stat_statements +
CloudWatch + Cost Explorer) → top queries by $/mo + breakeven math +
fix-list; then CI gate.
First experiment: 7-day join on 1 real high-I/O cluster.
Success threshold: ≥$500/mo or ≥20% of I/O bill attributed to specific
fixes. Kill threshold: >50% unattributed, or <10% savings across 3 clusters.
Product path: CLI + weekly cron → CI gate → FinOps seat/share-of-savings.
Main risk: AWS/Datadog absorption; thin moat; success-churn.
Confidence: medium-high (problem/first-step certain; commercial durability
unproven — hence fast kill criterion).

## Candidate: W2 — Restore proof + RTO measurement
Decision: RESEARCH

Problem: verify-green/restore-red (GitLab, Matrix). User: SRE/platform.
Evidence: catastrophic, low-frequency. Gap real (no proof layer).
Wedge: off-prod restore + WAL replay + smoke queries → PASS/FAIL + RTO.
Experiment: 5 WTP interviews + 1 pilot restore-proof before code.
Success: paid pilot or compliance-driven repeat intent. Kill: manual RTO <
SLO with gaps never firing for 30 days on pinned repos.
Product path: CLI + scheduled proof → compliance artifact.
Main risk: insurance GTM; scratch-restore economics at TB scale.
Confidence: medium (mechanism clear; market unproven).

## Candidate: F — Migration gate (simulate + rewrite)
Decision: RESEARCH

Problem: 15-year-stable lock/scan/backfill incident patterns. Evidence:
strong. Gap: narrow (detection commoditized; only simulate+rewrite counts).
Experiment: labeled incident corpus — beat squawk with auto-fix diffs
before building execution. Kill: no missed-by-squawk demo or no rewrite
adoption. Main risk: "another linter" perception; incumbents own CI.
Confidence: medium-low.

## Candidate: C — LSM stall explainer (offline only)
Decision: RESEARCH

Problem: 10–50s stalls, unbounded SA corners. Evidence: strong in LSM
shops. Gap: sliver (explainer; scheduling saturated, control refused).
Experiment: attribute documented stalls on 3 public LOG traces, no code
beyond parser. Kill: fails attribution or buyers are only hyperscalers.
Main risk: nobody pays for explainers; research-saturated space.
Confidence: low-medium.

## Dropped

- A WA attribution: coarse solved, precise impossible in cloud. DROP.
- B SSD-aware writes: engine/vendor territory, FDP non-portable. DROP.
- D storage-path profiler: causality unobservable; remainder commoditized. DROP.
- E design advisor: 30-yr autopsy — platforms only. DROP.

## Why W1 is the single BUILD

It is the only candidate combining high-frequency pain (B5), read-only
deployment (N1/G5), days-scale prototype (F5), and direct dollar
measurability (H5) with a fast, cheap kill criterion. W2's severity is
higher but its GTM is event-driven and its economics unproven — exactly
what RESEARCH validation is for. No winner was manufactured: four
candidates were dropped on evidence, and W1 ships with the three
hostile objections above pre-registered against it.
