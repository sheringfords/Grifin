# Grifin V1 architecture

Small, systems-oriented, zero-dependency (outside clap/serde) Rust binary.

```text
src/
  trace.rs       event schema + strict validation (fail loudly)
  rng.rs         SplitMix64 + Zipf tables (bit-identical across platforms)
  workload.rs    phase-based deterministic generator; specs parsed from
                 workloads/*.json (specs separate from execution code)
  policy.rs      Policy trait: on_access (advisory) + victim +
                 on_place/on_move/on_evict (notifications) + tracked
  policies/      lru, clock, arc, tinylfu, lirs, static_split, grifin (+mod registry)
  simulator.rs   replay kernel: owns residency/dirty/occupancy/costs;
                 verifies every policy answer; asserts tracked()==resident
                 every event; checked arithmetic throughout
  metrics.rs     counters, latency percentiles, J/J_total, CellStats
  experiment.rs  matrix runner + manifest/provenance + summary.json
  report.rs      SUMMARY.md rendered FROM summary.json (never hand-copied)
  main.rs        CLI: list | run | matrix | report
tests/
  policy_checks.rs  hand-computable traces (LRU/CLOCK/ARC-scan/TinyLFU-
                    admission/LIRS-bootstrap/static) + determinism
workloads/       w1..w8 JSON specs
docs/            RESEARCH.md EXPERIMENT.md ARCHITECTURE.md RESULTS.md
                 research/literature.md
```

## Key contracts

- **Simulator owns truth.** Policies advise (admit tier, promote flag,
  victim choice) and mirror placement via notifications. Any disagreement
  (non-resident victim, empty-tier None, count drift) panics with the
  policy name and sequence number.
- **Zero-cap redirect.** Admissions to a zero-capacity tier redirect to
  the other tier. This yields single-tier mode (T1=0) in which ARC/LRU/
  CLOCK/TinyLFU/LIRS reduce to their textbook single-level forms — the
  basis of the hand-computable tests.
- **Demote-preferred T0 eviction.** A T0 victim moves to T1 when T1 has
  room (migration cost); otherwise it is evicted to storage (writeback if
  dirty). Uniform across policies; documented in RESEARCH.md.
- **Determinism.** No wall-clock, no thread-rng, no HashMap-iteration-
  dependent decisions inside policies. Provenance (SHA, rustc, OS/CPU,
  seeds, spec hashes, tier config) lands in every manifest.
- **No silent fallbacks.** Unknown policy/spec keys, malformed traces,
  overflow, and empty victim sets are hard errors.

## Reproduce

```sh
cargo build --release
./target/release/grifin matrix --out results/v1-final --seeds 1,2,3,4,5
./target/release/grifin report --dir results/v1-final
```

Small smoke (CI): `cargo test` + release build + 2-workload mini matrix.
See `.github/workflows/ci.yml`.
