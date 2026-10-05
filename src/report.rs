//! Report generation: markdown tables rendered FROM result artifacts.
//!
//! Nothing here is hand-copied. `render_summary` reads `Summary` (loaded
//! from `summary.json`) and emits tables with median +/- 95% CI across
//! seeds, plus a head-to-head section (Grifin-full vs best strong baseline)
//! and an ablation section. Conclusions are NOT drawn here — see
//! `docs/RESULTS.md`, which quotes these tables.

use crate::experiment::Summary;

fn fmt_cell(median: f64, ci: f64, digits: usize) -> String {
    format!("{:.digits$} ± {:.digits$}", median, ci, digits = digits)
}

pub fn render_summary(s: &Summary) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Grifin experiment `{}`\n\n",
        s.manifest.experiment_id
    ));
    out.push_str(&format!(
        "- git: {} (dirty: {})\n- toolchain: {}\n- platform: {} / {} ({} cpus)\n\
         - date: {}\n- tiers: {}\n- J coeffs: write_w={} migr_w={} cpu_w={}\n\
         - seeds: {:?}, events/workload: ~{}, warmup: {}\n- policies: {}\n\n",
        s.manifest.git_sha,
        s.manifest.git_dirty,
        s.manifest.rustc_version,
        s.manifest.os,
        s.manifest.arch,
        s.manifest.cpu_count,
        s.manifest.timestamp_utc,
        s.manifest.tier_config,
        s.manifest.cost_coeffs.write_w,
        s.manifest.cost_coeffs.migr_w,
        s.manifest.cost_coeffs.cpu_w,
        s.manifest.seeds,
        s.manifest.events_per_workload,
        s.manifest.warmup_frac,
        s.manifest.policies.join(", "),
    ));
    out.push_str("All values are medians across seeds with 95% CI half-widths.\n\n");

    let workloads: Vec<String> = {
        let mut w: Vec<String> = s.cells.iter().map(|c| c.workload.clone()).collect();
        w.sort();
        w.dedup();
        w
    };
    let policies = &s.manifest.policies;

    // Per-workload full tables: mean latency.
    for w in &workloads {
        out.push_str(&format!(
            "## {w} — mean latency ns/access (median ± CI95)\n\n"
        ));
        out.push_str(
            "| policy | mean_lat | p99_lat | J | hit% | phys_writes | migrations | policy_ns |\n",
        );
        out.push_str("|---|---|---|---|---|---|---|---|\n");
        for p in policies {
            if let Some(c) = s.cells.iter().find(|c| &c.workload == w && &c.policy == p) {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {:.2}±{:.2} | {} | {} | {:.0}±{:.0} |\n",
                    p,
                    fmt_cell(c.mean_lat_ns.median, c.mean_lat_ns.ci95_halfwidth, 0),
                    fmt_cell(c.p99_lat_ns.median, c.p99_lat_ns.ci95_halfwidth, 0),
                    fmt_cell(c.j_score.median, c.j_score.ci95_halfwidth, 0),
                    c.hit_ratio.median * 100.0,
                    c.hit_ratio.ci95_halfwidth * 100.0,
                    fmt_cell(
                        c.physical_writes.median,
                        c.physical_writes.ci95_halfwidth,
                        0
                    ),
                    fmt_cell(c.migrations.median, c.migrations.ci95_halfwidth, 0),
                    c.policy_ns_per_access.median,
                    c.policy_ns_per_access.ci95_halfwidth,
                ));
            }
        }
        out.push('\n');
    }

    // Head-to-head: grifin-full vs best strong baseline per workload.
    out.push_str("## Head-to-head: grifin-full vs best strong baseline\n\n");
    out.push_str("Best baseline = min median J among {arc, tinylfu, lirs} per workload.\n\n");
    out.push_str("| workload | metric | grifin-full | best_baseline | delta% |\n");
    out.push_str("|---|---|---|---|---|\n");
    for w in &workloads {
        let g = s
            .cells
            .iter()
            .find(|c| &c.workload == w && c.policy == "grifin-full");
        let mut best: Option<&crate::experiment::CellSummary> = None;
        for b in ["arc", "tinylfu", "lirs"] {
            if let Some(c) = s.cells.iter().find(|c| &c.workload == w && c.policy == b) {
                if best
                    .map(|bc| c.j_score.median < bc.j_score.median)
                    .unwrap_or(true)
                {
                    best = Some(c);
                }
            }
        }
        if let (Some(g), Some(b)) = (g, best) {
            for (mname, gm, bm) in [
                ("mean_lat", g.mean_lat_ns.median, b.mean_lat_ns.median),
                ("p99_lat", g.p99_lat_ns.median, b.p99_lat_ns.median),
                ("J", g.j_score.median, b.j_score.median),
            ] {
                let d = if bm != 0.0 {
                    (gm - bm) / bm * 100.0
                } else {
                    0.0
                };
                out.push_str(&format!(
                    "| {w} | {mname} | {gm:.0} | {bm:.0} ({}) | {d:+.1}% |\n",
                    b.policy
                ));
            }
        }
    }
    out.push('\n');

    // Ablations.
    out.push_str("## Ablations (median J per workload)\n\n");
    out.push_str("| workload | grifin-reuse | +write | +relation | full | best_baseline_J |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for w in &workloads {
        let get = |p: &str| {
            s.cells
                .iter()
                .find(|c| &c.workload == w && c.policy == p)
                .map(|c| c.j_score.median)
        };
        let bb = ["arc", "tinylfu", "lirs"]
            .iter()
            .filter_map(|b| get(b))
            .fold(f64::INFINITY, f64::min);
        out.push_str(&format!(
            "| {w} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |\n",
            get("grifin-reuse").unwrap_or(f64::NAN),
            get("grifin-reuse-write").unwrap_or(f64::NAN),
            get("grifin-reuse-relation").unwrap_or(f64::NAN),
            get("grifin-full").unwrap_or(f64::NAN),
            bb,
        ));
    }
    out.push('\n');

    // Overhead.
    out.push_str("## Policy overhead (median ns/access, wall-clock)\n\n");
    out.push_str("| workload | ");
    for p in policies {
        out.push_str(&format!("{p} | "));
    }
    out.push_str("\n|---|");
    for _ in policies {
        out.push_str("---|");
    }
    out.push('\n');
    for w in &workloads {
        out.push_str(&format!("| {w} | "));
        for p in policies {
            if let Some(c) = s.cells.iter().find(|c| &c.workload == w && &c.policy == p) {
                out.push_str(&format!("{:.0} | ", c.policy_ns_per_access.median));
            } else {
                out.push_str("? | ");
            }
        }
        out.push('\n');
    }
    out
}
