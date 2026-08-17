mod config_contract;
mod config_format;
mod lineage_engine;
mod pipeline_lints;
mod reporter;
mod schema_emitter;
mod seed_registry;
mod sql_validator;
mod ui_model;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Parser, Debug)]
#[command(name = "thalweg-validate")]
struct Cli {
    /// Path to config.yaml / config.json / config.toml
    #[arg(long)]
    config: PathBuf,

    /// Directory containing referenced transform SQL files (paths in the
    /// config are relative to this).
    #[arg(long)]
    transforms_dir: PathBuf,

    /// Write the full JSON report to this path (in addition to the default
    /// human-readable summary printed to stdout).
    #[arg(long)]
    output: Option<PathBuf>,

    /// Print the full JSON report to stdout instead of the human-readable
    /// summary (e.g. for piping into `jq`).
    #[arg(long)]
    json: bool,

    /// Colorize the human-readable report. "auto" (default) colors only
    /// when stdout is a terminal - piping to a file or into another
    /// program gets plain text automatically.
    #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,
}

fn should_colorize(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => std::io::stdout().is_terminal(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = config_format::load(&cli.config, None)?;

    // Pipeline-level lints run *before* DAG construction: they're pure
    // functions of the parsed config (no SQL planning, no I/O), so they
    // still fire even when a downstream SQL error would have blocked the
    // per-table view. That lets a user with a broken SQL file also see a
    // fanout-risk warning on the same run instead of only after they've
    // fixed the SQL.
    let pipeline_report = pipeline_lints::run_pipeline_lints(&config);

    let nodes = lineage_engine::build_lineage(&config, &cli.transforms_dir)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tables = ui_model::build_report(&nodes);
    let failed = reporter::is_red(&tables) || pipeline_report.has_error();

    if cli.json {
        reporter::print_json(&tables, &pipeline_report)?;
    } else {
        reporter::print_human_readable(&tables, &pipeline_report, should_colorize(cli.color));
    }

    if let Some(path) = &cli.output {
        reporter::write_json_to_file(&tables, &pipeline_report, path)?;
    }

    if failed {
        std::process::exit(1);
    }

    Ok(())
}
