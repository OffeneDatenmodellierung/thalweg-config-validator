mod config_contract;
mod config_format;
mod lineage_engine;
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

    let nodes = lineage_engine::build_lineage(&config, &cli.transforms_dir)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tables = ui_model::build_report(&nodes);
    let failed = reporter::is_red(&tables);

    if cli.json {
        reporter::print_json(&tables)?;
    } else {
        reporter::print_human_readable(&tables, should_colorize(cli.color));
    }

    if let Some(path) = &cli.output {
        reporter::write_json_to_file(&tables, path)?;
    }

    if failed {
        std::process::exit(1);
    }

    Ok(())
}
