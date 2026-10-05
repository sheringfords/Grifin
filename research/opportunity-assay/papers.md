# Papers (evidence classes: [P] verified peer-reviewed · [P2] per secondary source · [W] workshop · [E] eng/code/docs · [A] anecdote)

## Write amplification / LSM / SSD

- Dong et al., Evolution of Development Priorities in RocksDB, FAST'21 [P] —
  https://www.usenix.org/system/files/fast21-dong.pdf — 42 prod apps/100s PB;
  leveled WA 10–30, tiered 4–10; SSD-internal WA 1.1–3 observed. By end of
  study space/CPU eclipsed WA as top concern (kill-use for WA-centrism).
- Cao et al., Characterizing RocksDB at Facebook, FAST'20 [P] —
  https://www.usenix.org/system/files/fast20-cao_zhichao.pdf — YCSB
  misestimates prod I/O (7.7× reads); underestimates WA.
- Qiao et al., Transparent Compression closing B-tree/LSM WA gap, FAST'22 [P] —
  https://www.usenix.org/system/files/fast22-qiao.pdf — needs compute-SSD HW.
- Oh et al., exF2FS, FAST'22 [P] —
  https://www.usenix.org/system/files/fast22-oh.pdf — SQLite 1 insert =
  5×fdatasync + 40KB; FS-level txn offload (requires app API change).
- Maneas et al., NetApp SSD field study, FAST'22 [P] —
  https://www.usenix.org/system/files/fast22-maneas.pdf — ~95% of enterprise
  drives could have survived QLC; endurance rarely binds (kill-use).
- Mohan et al., IO Amplification in Linux FS, arXiv:1707.08514 [P2/preprint] —
  https://arxiv.org/abs/1707.08514 — ext4 overwrite 4.0×, btrfs 32.65×.
- Lee/Ziegler/Leis, How to Write to SSDs, PVLDB'26 [P] —
  https://arxiv.org/abs/2603.09927 — in-place total WAF ≈4.7 (DB 2.0 × SSD
  2.36); out-of-place ZLeanStore −6.2–9.8× flash writes, +1.65–2.45× TPS;
  LeanStore 400MB/s vs 11MB/s budget on PM9A3-90%-full. No independent repro.
- Haas/Leis, What Modern NVMe Can Do, PVLDB'23 [P] —
  https://vldb.org/pvldb/vol16/p2090-haas.pdf — fresh-SSD numbers collapse
  when full (datasheet worst-case 135k IOPS).
- Song et al., WARP (open FDP emulator), FAST'26 [P] —
  https://www.usenix.org/system/files/fast26-song.pdf — FDP ≈1.0 WAF iff RUH
  isolation aligns; adversarial 3-stream 4.49×/2.58× per drive; vendor-divergent
  best-effort firmware (kill-use for portable FDP product).
- Balmau et al., SILK, FAST'19 [P], DOI 10.1145/3380905 — ~2× tail cut via
  I/O partitioning; research fork, never upstreamed.
- Lu et al., ADOC, FAST'20 [P] — ~2× sustained throughput, near-zero stalls;
  db_bench only, closed code, never upstreamed (the "auto-tune" already published).
- Xanthakis et al., vLSM, arXiv:2407.15581 [P2/preprint] — P99 write 4.8×,
  read 12.5× via compaction chains; RocksDB fork.
- Dayan/Idreos, Dostoevsky, SIGMOD'18 [P], DOI 10.1145/3183713.3196927 —
  analytic model, not online scheduler.
- CaaS-LSM (compaction-as-a-service), SIGMOD'24 [P], DOI 10.1145/3654927 —
  needs disaggregated infra.

## Physical design / tuning

- AutoAdmin lineage (MSR 1996–2006): Index Tuning Wizard (VLDB97, 10-yr best
  paper), DTA (VLDB04 —
  https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/VLDB04.pdf).
  Order-of-magnitude speedups claimed — inside the platform.
- Azure auto-indexing at scale [E/paper] —
  https://www.microsoft.com/en-us/research/wp-content/uploads/2019/02/autoindexing_azuredb.pdf —
  2+ yrs GA, 100ks of DBs: worked because MS owned apply+validate+revert.
- Budget-aware index tuning (RL/MCTS), SIGMOD22 —
  https://www.microsoft.com/en-us/research/wp-content/uploads/2022/06/mcts-full.pdf —
  optimises what-if calls, not adoption.
- Self-tuning DBs survey, ACM CSUR 2024, DOI 10.1145/3665323 — "all major
  vendors ship automated physical design tools" — research continues because
  deployment didn't.
- OtterTune: dead 2024 ($14.6M raised; knob-ML, no willingness-to-pay).
  EverSQL: acqui-hired into Aiven Nov 2023 (standalone → platform feature).

## Observability / diagnosis

- No [P] found claiming SQL→relation→buffer/WAL→FS→block→device causal
  attribution on stock Linux + cloud EBS after explicit FAST/OSDI/EuroSys
  search — absence is evidence.
- pg_stat_io (PG16+, PG18 adds bytes/WAL/per-backend): aggregate counters,
  no query ID; timing off by default (overhead). MySQL PFS: file-level waits,
  sampled/lossy. RocksDB PerfContext: stops at file reads. OTel DB semconv:
  query duration only; collectors poll at 10–60s.

## Verdicts encoded here

WA-centrism is empirically weak for most fleets (Dong, Maneas); FDP wins are
vendor-divergent (WARP); compaction scheduling is a saturated design space
(≥10 unmerged schedulers); advisors monetise only inside platforms; causal
cross-layer attribution has no published existence proof.
