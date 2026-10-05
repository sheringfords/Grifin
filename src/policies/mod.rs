//! Policy registry: every policy behind the same constructor.

use crate::policies::arc::Arc;
use crate::policies::clock::Clock;
use crate::policies::grifin::{Grifin, GrifinFlags};
use crate::policies::lirs::Lirs;
use crate::policies::lru::Lru;
use crate::policies::static_split::StaticSplit;
use crate::policies::tinylfu::TinyLfu;
use crate::policy::{Policy, PolicyCtx};

pub mod arc;
pub mod clock;
pub mod grifin;
pub mod lirs;
pub mod lru;
pub mod static_split;
pub mod tinylfu;

/// All policy names runnable in the experiment matrix.
pub fn all_names() -> Vec<&'static str> {
    vec![
        "lru",
        "clock",
        "arc",
        "tinylfu",
        "lirs",
        "static",
        "grifin-reuse",
        "grifin-reuse-write",
        "grifin-reuse-relation",
        "grifin-full",
    ]
}

/// Baselines that Grifin must beat (strong adaptive controls).
pub fn strong_baselines() -> Vec<&'static str> {
    vec!["arc", "tinylfu", "lirs"]
}

pub fn make(name: &str, ctx: PolicyCtx) -> Result<Box<dyn Policy>, String> {
    match name {
        "lru" => Ok(Box::new(Lru::new(ctx))),
        "clock" => Ok(Box::new(Clock::new(ctx))),
        "arc" => Ok(Box::new(Arc::new(ctx))),
        "tinylfu" => Ok(Box::new(TinyLfu::new(ctx))),
        "lirs" => Ok(Box::new(Lirs::new(ctx))),
        "static" => Ok(Box::new(StaticSplit::new(ctx))),
        "grifin-reuse" => Ok(Box::new(Grifin::new(ctx, GrifinFlags::reuse_only()))),
        "grifin-reuse-write" => Ok(Box::new(Grifin::new(ctx, GrifinFlags::reuse_write()))),
        "grifin-reuse-relation" => Ok(Box::new(Grifin::new(ctx, GrifinFlags::reuse_relation()))),
        "grifin-full" => Ok(Box::new(Grifin::new(ctx, GrifinFlags::full()))),
        other => Err(format!("unknown policy: {other}")),
    }
}
