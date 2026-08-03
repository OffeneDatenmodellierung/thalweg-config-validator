//! Renders the final table view models either as a human-readable CLI
//! report (default, to stdout, with optional ANSI color) or as JSON (to a
//! file via --output, or to stdout via --json).

use crate::ui_model::TableViewModel;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::Path;

#[derive(serde::Serialize)]
struct Report {
    overall_status: String,
    tables: Vec<TableViewModel>,
}

pub fn is_red(tables: &[TableViewModel]) -> bool {
    tables.iter().any(|t| t.banner == "red")
}

fn overall_status(tables: &[TableViewModel]) -> String {
    if is_red(tables) {
        "red".to_string()
    } else {
        "green".to_string()
    }
}

/// Wraps `text` in the given ANSI SGR code if `color` is enabled, otherwise
/// returns it unchanged. Hand-rolled rather than pulling in a color crate -
/// this is the entire extent of what's needed here.
fn colorize(text: &str, sgr_code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{sgr_code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

const GREEN: &str = "32";
const RED: &str = "31";
const YELLOW: &str = "33";
const DIM: &str = "2";
const BOLD: &str = "1";

fn banner_display(banner: &str, color: bool) -> String {
    match banner {
        "green" => colorize("[OK]  ", GREEN, color),
        "red" => colorize("[FAIL]", RED, color),
        "virtual" => colorize("[--]  ", DIM, color),
        _ => "[?]   ".to_string(),
    }
}

fn severity_color(severity: &str) -> &'static str {
    match severity {
        "error" => RED,
        "warning" => YELLOW,
        _ => "",
    }
}

/// Renders a human-readable report: one block per table in DAG order, each
/// showing its banner, input/output-table linkage, findings, and any
/// untraceable columns. `color` controls ANSI SGR codes - plain ASCII text
/// and structure either way, so it still renders sanely with color off
/// (CI logs, redirected output, non-color terminals).
pub fn render_human_readable(tables: &[TableViewModel], color: bool) -> String {
    let mut out = String::new();
    let status = overall_status(tables).to_uppercase();
    let status_colored = colorize(&status, if status == "RED" { RED } else { GREEN }, color);
    let _ = writeln!(out, "Pipeline validation: {status_colored}\n");

    for t in tables {
        let mut header = format!("{} {}", banner_display(&t.banner, color), colorize(&t.name, BOLD, color));
        if t.is_primary {
            header.push_str(" (primary transform)");
        } else if t.is_virtual {
            header.push_str(" (virtual)");
        }
        if let Some(ct) = &t.clean_table {
            let _ = write!(header, " -> {ct}");
        }
        let _ = writeln!(out, "{header}");

        if let Some(input) = &t.input {
            let _ = writeln!(out, "    input: {input}");
        }

        if t.findings.is_empty() {
            let _ = writeln!(out, "    no findings");
        } else {
            for f in &t.findings {
                let tag = colorize(
                    &format!("[{}/{}]", f.severity, f.category),
                    severity_color(&f.severity),
                    color,
                );
                let _ = writeln!(out, "    - {tag} {}", f.message);
            }
        }

        let untraceable: Vec<&str> = t
            .columns
            .iter()
            .filter(|c| !c.traceable)
            .map(|c| c.name.as_str())
            .collect();
        if !untraceable.is_empty() {
            let label = colorize("untraceable columns:", YELLOW, color);
            let _ = writeln!(out, "    {label} {}", untraceable.join(", "));
        }

        out.push('\n');
    }

    out
}

pub fn print_human_readable(tables: &[TableViewModel], color: bool) {
    print!("{}", render_human_readable(tables, color));
}

fn to_json(tables: &[TableViewModel]) -> Result<String> {
    let report = Report {
        overall_status: overall_status(tables),
        tables: tables.to_vec(),
    };
    serde_json::to_string_pretty(&report).context("serializing report to JSON")
}

pub fn print_json(tables: &[TableViewModel]) -> Result<()> {
    println!("{}", to_json(tables)?);
    Ok(())
}

pub fn write_json_to_file(tables: &[TableViewModel], path: &Path) -> Result<()> {
    let json = to_json(tables)?;
    std::fs::write(path, json)
        .with_context(|| format!("writing report to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tables() -> Vec<TableViewModel> {
        vec![
            TableViewModel {
                name: "a".to_string(),
                banner: "green".to_string(),
                is_virtual: false,
                is_primary: false,
                input: Some("prepared".to_string()),
                clean_table: None,
                quarantine_table: None,
                columns: vec![],
                findings: vec![],
                ddl: None,
            },
            TableViewModel {
                name: "b".to_string(),
                banner: "red".to_string(),
                is_virtual: false,
                is_primary: false,
                input: Some("prepared".to_string()),
                clean_table: None,
                quarantine_table: None,
                columns: vec![],
                findings: vec![],
                ddl: None,
            },
        ]
    }

    #[test]
    fn overall_status_is_red_if_any_table_is_red() {
        let tables = sample_tables();
        assert!(is_red(&tables));
        assert_eq!(overall_status(&tables), "red");
    }

    #[test]
    fn human_readable_report_mentions_every_table_and_overall_status() {
        let tables = sample_tables();
        let rendered = render_human_readable(&tables, false);
        assert!(rendered.contains("RED"));
        assert!(rendered.contains("a\n"));
        assert!(rendered.contains("b\n"));
        // No color -> no ANSI escape bytes.
        assert!(!rendered.contains('\x1b'));
    }

    #[test]
    fn color_mode_emits_ansi_escape_codes() {
        let tables = sample_tables();
        let rendered = render_human_readable(&tables, true);
        assert!(rendered.contains('\x1b'));
    }
}
