# Grifin

> **Grifin is an experimental database-storage research project investigating
> whether database semantics can improve data placement across modern memory
> and storage hierarchies. It is experimental: there is no product here.**

## Research question

> Can database-semantic information improve page admission, retention, and
> placement decisions enough to beat strong generic caching policies after
> accounting for policy overhead, migrations, write traffic, and changing
> workloads?

## Current hypothesis

Engines know things the block layer does not (relation identity, page type,
dirty state, update frequency, lifetime). Grifin V0 prices those signals
into an explicit, interpretable per-page value function — no ML — and we
measure whether it survives contact with ARC, TinyLFU, LIRS, CLOCK, and
adversarial phase changes. See `docs/RESEARCH.md`.

## Status

V1 foundation in progress on `research/storage-policy-foundation-v1`.
Current result: see `docs/RESULTS.md` (exactly one of
KEEP / WEAK_KEEP / KILL / INCONCLUSIVE once the final matrix lands).

## Reproduce the experiment

```sh
cargo build --release
./target/release/grifin matrix --out results/v1-final --seeds 1,2,3,4,5
./target/release/grifin report --dir results/v1-final
```

## Layout

- `src/` — deterministic simulator + 10 policy configs (6 baselines +
  4 Grifin ablations), zero production dependencies beyond clap/serde.
- `workloads/` — 8 deterministic specs (stable, scan pollution, shift,
  write burst, mixed relations, churn, analytical, adversarial).
- `docs/` — `RESEARCH.md` (frozen hypothesis), `EXPERIMENT.md`
  (pre-registered gate), `ARCHITECTURE.md`, `RESULTS.md`,
  `research/literature.md`.
- `results/` — machine-readable artifacts (git-ignored except committed
  summaries).

## Limitations (honest)

- Simulation, not hardware: tiers are parameters; no CXL/ZNS/FDP/kernel work.
- Synthetic traces first; no production trace validation yet in V1.
- CLOCK-Pro transcribed via its ingredients, not directly (documented).
- Fixed-weight value model; no learning (by design — see LeCaR note in
  `docs/research/literature.md`).
