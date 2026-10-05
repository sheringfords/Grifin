//! CLI: `run` a single cell, `matrix` the full suite, `report` re-render tables.

use clap::{Parser, Subcommand};
use grifin::experiment::{run_matrix, run_single, MatrixConfig};
use grifin::policies;
use grifin::policy::PolicyCtx;
use grifin::simulator::SimSpec;
use grifin::workload::load_spec;

#[derive(Parser)]
#[command(
    name = "grifin",
    about = "Grifin V1 storage-placement simulator (research prototype)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List policies and workloads.
    List,
    /// Run one (workload, policy, seed) cell and print JSON.
    Run {
        #[arg(long)]
        workload: String,
        #[arg(long)]
        policy: String,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 60000)]
        events: u64,
    },
    /// Run the full matrix and write results/<id>/ artifacts.
    Matrix {
        #[arg(long, default_value = "results/v1-final")]
        out: String,
        #[arg(long, default_value = "1,2,3,4,5")]
        seeds: String,
        #[arg(long, default_value_t = 60000)]
        events: u64,
        #[arg(long, default_value = "workloads")]
        workload_dir: String,
        /// Comma-separated policy subset (default: all).
        #[arg(long, default_value = "")]
        policies: String,
    },
    /// Re-render SUMMARY.md from an existing summary.json.
    Report {
        #[arg(long)]
        dir: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            println!("policies:");
            for p in policies::all_names() {
                println!("  {p}");
            }
            println!("workloads (in ./workloads):");
            let mut v: Vec<_> = std::fs::read_dir("workloads")
                .expect("no workloads dir")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".json"))
                .collect();
            v.sort();
            for n in v {
                println!("  {n}");
            }
        }
        Cmd::Run {
            workload,
            policy,
            seed,
            events,
        } => {
            let spec = load_spec(&workload).unwrap_or_else(|e| panic!("spec error: {e}"));
            let scaled = spec.scaled(events);
            let sim = SimSpec::default_hierarchy();
            let rec = run_single(&scaled, &policy, seed, &sim);
            println!("{}", serde_json::to_string_pretty(&rec).unwrap());
        }
        Cmd::Matrix {
            out,
            seeds,
            events,
            workload_dir,
            policies,
        } => {
            let seeds: Vec<u64> = seeds
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<u64>()
                        .unwrap_or_else(|_| panic!("bad seed: {s}"))
                })
                .collect();
            assert!(!seeds.is_empty(), "need at least one seed");
            let all = policies::all_names()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            let subset: Vec<String> = if policies.is_empty() {
                all
            } else {
                let want: Vec<&str> = policies.split(',').map(|s| s.trim()).collect();
                for w in &want {
                    assert!(policies::all_names().contains(w), "unknown policy: {w}");
                }
                // Validate ctx construction for each.
                for w in &want {
                    let _ = policies::make(
                        w,
                        PolicyCtx {
                            t0_cap: 8,
                            t1_cap: 8,
                        },
                    )
                    .unwrap();
                }
                want.iter().map(|s| s.to_string()).collect()
            };
            let id = std::path::Path::new(&out)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let cfg = MatrixConfig {
                out_dir: out,
                workload_dir,
                seeds,
                events,
                sim: SimSpec::default_hierarchy(),
                policies: subset,
                experiment_id: id,
            };
            let summary = run_matrix(&cfg);
            println!("wrote {} cells to {}", summary.cells.len(), cfg.out_dir);
        }
        Cmd::Report { dir } => {
            let raw = std::fs::read_to_string(format!("{dir}/summary.json"))
                .expect("cannot read summary.json");
            let summary: grifin::experiment::Summary =
                serde_json::from_str(&raw).expect("cannot parse summary.json");
            let md = grifin::report::render_summary(&summary);
            std::fs::write(format!("{dir}/SUMMARY.md"), md).expect("write failed");
            println!("re-rendered {dir}/SUMMARY.md");
        }
    }
}
