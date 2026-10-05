//! Grifin V1: deterministic simulator for database-aware storage placement.
//!
//! Research prototype. See `docs/RESEARCH.md` and `docs/EXPERIMENT.md`.

pub mod experiment;
pub mod metrics;
pub mod policies;
pub mod policy;
pub mod report;
pub mod rng;
pub mod simulator;
pub mod trace;
pub mod workload;
