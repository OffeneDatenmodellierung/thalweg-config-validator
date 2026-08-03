//! View-model structs consumed by whatever renders validation results (CLI
//! text/JSON output today; a future TUI/web UI could consume the same JSON
//! via `reporter`).

use crate::lineage_engine::{Banner, LineageNode};
use crate::schema_emitter;
use crate::sql_validator::{Category, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ColumnViewModel {
    pub name: String,
    pub traceable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingViewModel {
    pub severity: String,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableViewModel {
    pub name: String,
    pub banner: String,
    pub is_virtual: bool,
    pub is_primary: bool,
    pub input: Option<String>,
    pub clean_table: Option<String>,
    pub quarantine_table: Option<String>,
    pub columns: Vec<ColumnViewModel>,
    pub findings: Vec<FindingViewModel>,
    pub ddl: Option<String>,
}

fn banner_label(b: Banner) -> &'static str {
    match b {
        Banner::Green => "green",
        Banner::Red => "red",
        Banner::Virtual => "virtual",
    }
}

fn severity_label(s: &Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn category_label(c: &Category) -> &'static str {
    match c {
        Category::Syntax => "syntax",
        Category::Rule => "rule",
        Category::Schema => "schema",
    }
}

pub fn build_table_view(node: &LineageNode) -> TableViewModel {
    TableViewModel {
        name: node.name.clone(),
        banner: banner_label(node.banner).to_string(),
        is_virtual: node.is_virtual,
        is_primary: node.is_primary,
        input: node.input.clone(),
        clean_table: node.clean_table.clone(),
        quarantine_table: node.quarantine_table.clone(),
        columns: node
            .column_origins
            .iter()
            .map(|c| ColumnViewModel {
                name: c.name.clone(),
                traceable: c.traceable,
            })
            .collect(),
        findings: node
            .findings
            .iter()
            .map(|f| FindingViewModel {
                severity: severity_label(&f.severity).to_string(),
                category: category_label(&f.category).to_string(),
                message: f.message.clone(),
            })
            .collect(),
        ddl: schema_emitter::emit_ddl(node),
    }
}

pub fn build_report(nodes: &[LineageNode]) -> Vec<TableViewModel> {
    nodes.iter().map(build_table_view).collect()
}
