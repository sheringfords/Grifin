//! Trace schema: the minimal event model from `docs/RESEARCH.md`.
//!
//! A trace is an ordered sequence of page accesses. Optional metadata is
//! genuinely optional: policies must behave sensibly when it is absent, and
//! the experiment never makes Grifin win by withholding generic signals
//! (recency/frequency) from baselines while giving them to Grifin.

use serde::{Deserialize, Serialize};

pub type PageId = u64;

/// Operation on a page. WRITE marks the resident copy dirty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Read,
    Write,
}

impl Op {
    pub fn is_write(self) -> bool {
        matches!(self, Op::Write)
    }
}

/// Database page type. Synthetic workloads assign these by position so that
/// page-type signals are honest (inner pages are genuinely hotter because
/// the generator routes tree traversals through them, not because we assert it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageType {
    Meta,
    Inner,
    Leaf,
    Heap,
    Blob,
}

/// One access to a database page/object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Position in the trace (also serves as logical timestamp).
    pub seq: u64,
    pub page_id: PageId,
    /// Relation (table/index) the page belongs to.
    pub relation_id: u32,
    pub op: Op,
    pub page_type: PageType,
    pub txn_id: u64,
    pub size_bytes: u32,
    // ---- optional metadata (may be None) ----
    /// Relation kind, e.g. "btree", "heap", "toast". Free-form on purpose.
    pub relation_kind: Option<String>,
    /// Logical group inside the relation (e.g. key-range shard).
    pub logical_group: Option<u32>,
    /// Event index at which the page was created (lifetime analysis).
    pub creation_time: Option<u64>,
}

impl TraceEvent {
    /// Strict validation: malformed traces must fail loudly, never be silently clamped.
    pub fn validate(&self) -> Result<(), String> {
        if self.size_bytes == 0 {
            return Err(format!("event {}: size_bytes must be > 0", self.seq));
        }
        if self.size_bytes > 1_048_576 {
            return Err(format!(
                "event {}: size_bytes {} exceeds 1MiB sanity bound",
                self.seq, self.size_bytes
            ));
        }
        Ok(())
    }
}

/// Validate a whole trace: strictly increasing seq starting at 0.
pub fn validate_trace(trace: &[TraceEvent]) -> Result<(), String> {
    for (i, ev) in trace.iter().enumerate() {
        if ev.seq != i as u64 {
            return Err(format!(
                "trace corrupt: event at index {} has seq {}",
                i, ev.seq
            ));
        }
        ev.validate()?;
    }
    Ok(())
}
