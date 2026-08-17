//! Renders the final table view models either as a human-readable CLI
//! report (default, to stdout, with optional ANSI color) or as JSON (to a
//! file via --output, or to stdout via --json).
//!
//! Reports carry two layers:
//!
//! 1. Pipeline-level lints from `pipeline_lints` (config-wide invariants
//!    that don't belong to any single node — e.g. batch-sizing vs.
//!    fanout-explode risk).
//! 2. Per-table findings from `lineage_engine` + `sql_validator`.
//!
//! Rendered in that order because a pipeline-level Error will typically
//! reframe how a reader interprets the per-table findings underneath it.

use crate::pipeline_lints::{PipelineFinding, PipelineLintReport, PipelineSeverity};
use crate::ui_model::TableViewModel;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::Path;

#[derive(serde::Serialize)]
struct Report<'a> {
    overall_status: String,
    pipeline_lints: &'a [PipelineFinding],
    tables: &'a [TableViewModel],
}

pub fn is_red(tables: &[TableViewModel]) -> bool {
    tables.iter().any(|t| t.banner == "red")
}

fn overall_status(tables: &[TableViewModel], pipeline: &PipelineLintReport) -> String {
    if is_red(tables) || pipeline.has_error() {
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

fn pipeline_severity_color(severity: PipelineSeverity) -> &'static str {
    match severity {
        PipelineSeverity::Error => RED,
        PipelineSeverity::Warning => YELLOW,
    }
}

fn pipeline_severity_label(severity: PipelineSeverity) -> &'static str {
    match severity {
        PipelineSeverity::Error => "error",
        PipelineSeverity::Warning => "warning",
    }
}

fn render_pipeline_lints(pipeline: &PipelineLintReport, color: bool, out: &mut String) {
    if pipeline.is_empty() {
        return;
    }
    let header = colorize("Pipeline-level lints:", BOLD, color);
    let _ = writeln!(out, "{header}");
    for f in &pipeline.findings {
        // Each pipeline finding carries its own stable LintId — surface
        // it so operators can grep the issue tracker / docs by that ID
        // rather than by message text (which may be reworded over time).
        let id_slug = serde_json::to_value(f.id)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        let tag = colorize(
            &format!("[{}/{}]", pipeline_severity_label(f.severity), id_slug),
            pipeline_severity_color(f.severity),
            color,
        );
        let _ = writeln!(out, "  - {tag} {}", f.message);
        if !f.related_nodes.is_empty() {
            let label = colorize("related nodes:", DIM, color);
            let _ = writeln!(out, "    {label} {}", f.related_nodes.join(", "));
        }
    }
    out.push('\n');
}

/// Renders a human-readable report: pipeline-level lints (if any) followed
/// by one block per table in DAG order, each showing its banner,
/// input/output-table linkage, findings, and any untraceable columns.
/// `color` controls ANSI SGR codes - plain ASCII text and structure
/// either way, so it still renders sanely with color off (CI logs,
/// redirected output, non-color terminals).
pub fn render_human_readable(
    tables: &[TableViewModel],
    pipeline: &PipelineLintReport,
    color: bool,
) -> String {
    let mut out = String::new();
    let status = overall_status(tables, pipeline).to_uppercase();
    let status_colored = colorize(&status, if status == "RED" { RED } else { GREEN }, color);
    let _ = writeln!(out, "Pipeline validation: {status_colored}\n");

    render_pipeline_lints(pipeline, color, &mut out);

    for t in tables {
        let mut header = format!(
            "{} {}",
            banner_display(&t.banner, color),
            colorize(&t.name, BOLD, color)
        );
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

pub fn print_human_readable(tables: &[TableViewModel], pipeline: &PipelineLintReport, color: bool) {
    print!("{}", render_human_readable(tables, pipeline, color));
}

fn to_json(tables: &[TableViewModel], pipeline: &PipelineLintReport) -> Result<String> {
    let report = Report {
        overall_status: overall_status(tables, pipeline),
        pipeline_lints: &pipeline.findings,
        tables,
    };
    serde_json::to_string_pretty(&report).context("serializing report to JSON")
}

pub fn print_json(tables: &[TableViewModel], pipeline: &PipelineLintReport) -> Result<()> {
    println!("{}", to_json(tables, pipeline)?);
    Ok(())
}

pub fn write_json_to_file(
    tables: &[TableViewModel],
    pipeline: &PipelineLintReport,
    path: &Path,
) -> Result<()> {
    let json = to_json(tables, pipeline)?;
    std::fs::write(path, json).with_context(|| format!("writing report to {}", path.display()))?;
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

    fn empty_pipeline() -> PipelineLintReport {
        PipelineLintReport::default()
    }

    #[test]
    fn overall_status_is_red_if_any_table_is_red() {
        let tables = sample_tables();
        let pipeline = empty_pipeline();
        assert!(is_red(&tables));
        assert_eq!(overall_status(&tables, &pipeline), "red");
    }

    #[test]
    fn overall_status_is_red_if_pipeline_lint_has_error_even_with_all_green_tables() {
        // Guards a subtle wiring hazard: pipeline-level Errors must exit
        // non-zero and drive the overall status red even when every
        // per-table banner is green.
        let tables: Vec<TableViewModel> = vec![TableViewModel {
            name: "green_only".to_string(),
            banner: "green".to_string(),
            is_virtual: false,
            is_primary: false,
            input: Some("base".to_string()),
            clean_table: Some("t_clean".to_string()),
            quarantine_table: None,
            columns: vec![],
            findings: vec![],
            ddl: None,
        }];
        let pipeline = PipelineLintReport {
            findings: vec![PipelineFinding {
                id: crate::pipeline_lints::LintId::ArrowI32FanoutRisk,
                severity: PipelineSeverity::Error,
                message: "synthetic".to_string(),
                related_nodes: vec![],
            }],
        };
        assert!(!is_red(&tables));
        assert_eq!(overall_status(&tables, &pipeline), "red");
    }

    #[test]
    fn human_readable_report_mentions_every_table_and_overall_status() {
        let tables = sample_tables();
        let pipeline = empty_pipeline();
        let rendered = render_human_readable(&tables, &pipeline, false);
        assert!(rendered.contains("RED"));
        assert!(rendered.contains("a\n"));
        assert!(rendered.contains("b\n"));
        // No color -> no ANSI escape bytes.
        assert!(!rendered.contains('\x1b'));
    }

    #[test]
    fn color_mode_emits_ansi_escape_codes() {
        let tables = sample_tables();
        let pipeline = empty_pipeline();
        let rendered = render_human_readable(&tables, &pipeline, true);
        assert!(rendered.contains('\x1b'));
    }

    #[test]
    fn pipeline_lints_render_above_per_table_findings_with_lint_id_visible() {
        // The lint ID (stable slug) must be surfaced in the rendered text
        // so operators can grep the framework issue tracker / docs by ID
        // instead of the message body. Fragile message-text matching in
        // consumer scripts is exactly what we want to avoid.
        let tables = sample_tables();
        let pipeline = PipelineLintReport {
            findings: vec![PipelineFinding {
                id: crate::pipeline_lints::LintId::ArrowI32FanoutRisk,
                severity: PipelineSeverity::Warning,
                message: "synthetic message body".to_string(),
                related_nodes: vec!["markets_virtual".to_string()],
            }],
        };
        let rendered = render_human_readable(&tables, &pipeline, false);
        assert!(
            rendered.contains("Pipeline-level lints:"),
            "section header missing"
        );
        assert!(
            rendered.contains("arrow-i32-fanout-risk"),
            "lint ID slug missing"
        );
        assert!(
            rendered.contains("markets_virtual"),
            "related node not surfaced"
        );

        // Ordering: pipeline lint block appears before the per-table
        // "a" block. render_human_readable prints pipeline lints, then
        // the per-table blocks in DAG order.
        let lint_pos = rendered.find("Pipeline-level lints:").unwrap();
        let table_a_pos = rendered.find("a\n").unwrap();
        assert!(
            lint_pos < table_a_pos,
            "pipeline lints must render before per-table blocks"
        );
    }

    #[test]
    fn json_report_carries_pipeline_lints_array_and_overall_status() {
        let tables = sample_tables();
        let pipeline = PipelineLintReport {
            findings: vec![PipelineFinding {
                id: crate::pipeline_lints::LintId::ArrowI32FanoutRisk,
                severity: PipelineSeverity::Warning,
                message: "synthetic".to_string(),
                related_nodes: vec!["markets_virtual".to_string()],
            }],
        };
        let json = to_json(&tables, &pipeline).unwrap();
        // Contract for downstream tooling: top-level `pipeline_lints` key
        // is stable, findings serialize with a kebab-case `id`.
        assert!(
            json.contains("\"pipeline_lints\":"),
            "top-level pipeline_lints key missing"
        );
        assert!(
            json.contains("\"arrow-i32-fanout-risk\""),
            "lint id kebab-case slug missing"
        );
        assert!(
            json.contains("\"overall_status\":"),
            "overall_status key missing"
        );
    }
}
