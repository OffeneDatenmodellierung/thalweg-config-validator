//! Validates a single transform's SQL in isolation: parses + logical-plans
//! it against a supplied upstream schema (no data, no execution), enforces
//! the no-JOIN rule, and classifies failures per `missing_column_mode`.
//!
//! Deliberately does NOT do DAG-aware lineage tracing (e.g. the
//! `untraceable_column.sql` case) - that requires the full sub_transforms
//! graph and lives in `lineage_engine` instead. This module only answers:
//! "does this SQL plan, and does it break a structural rule?"

use crate::config_contract::MissingColumnMode;
use datafusion::arrow::datatypes::{DataType, SchemaRef, TimeUnit};
use datafusion::datasource::MemTable;
use datafusion::logical_expr::LogicalPlan;
use datafusion::prelude::SessionContext;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Category {
    Syntax,
    Rule,
    Schema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub category: Category,
    pub message: String,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub findings: Vec<Finding>,
    /// Present only when the SQL planned successfully - used by
    /// lineage_engine (Phase 2) to inspect the output schema and expression
    /// tree. `None` on any hard failure (syntax, rule, or unresolvable
    /// schema error).
    pub plan: Option<LogicalPlan>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }
}

#[derive(Debug, Error)]
pub enum SqlValidatorError {
    #[error("failed to build in-memory table for schema registration: {0}")]
    SchemaSetup(String),
}

/// One table's schema, as seen by the transform under validation - either
/// the seed registry + primary transform's schema_hint_columns (for the
/// `base` transform), or a prior transform's inferred output schema (for
/// everything downstream, supplied by lineage_engine).
#[derive(Debug, Clone)]
pub struct UpstreamTable {
    pub name: String,
    pub schema: SchemaRef,
}

/// Parse the handful of Arrow type strings used in seed_registry and the
/// isolated test fixtures. NOT a general Arrow type-string parser - extend
/// as new types show up in real schema_hint_columns declarations.
pub fn parse_arrow_type(raw: &str) -> DataType {
    match raw {
        "Utf8" => DataType::Utf8,
        "Binary" => DataType::Binary,
        "Boolean" => DataType::Boolean,
        "Int32" => DataType::Int32,
        "Int64" => DataType::Int64,
        "Float64" => DataType::Float64,
        "Timestamp(Nanosecond, None)" => DataType::Timestamp(TimeUnit::Nanosecond, None),
        // schema_hint_columns uses these SQL-ish type names directly.
        "STRING" => DataType::Utf8,
        "BIGINT" => DataType::Int64,
        "INT" => DataType::Int32,
        other => {
            // Unknown type name: fall back to Utf8 rather than panicking, and
            // let the caller's diagnostics surface the raw string separately
            // if this matters. A stricter mode could hard-error here instead.
            tracing_unused_type_warning(other);
            DataType::Utf8
        }
    }
}

fn tracing_unused_type_warning(_raw: &str) {
    // Placeholder for real logging once a logging crate is wired in;
    // deliberately not panicking so unfamiliar type strings degrade to a
    // permissive default instead of blocking validation entirely.
}

fn register_tables(
    ctx: &SessionContext,
    tables: &[UpstreamTable],
) -> Result<(), SqlValidatorError> {
    for t in tables {
        let mem_table = MemTable::try_new(t.schema.clone(), vec![vec![]])
            .map_err(|e| SqlValidatorError::SchemaSetup(e.to_string()))?;
        ctx.register_table(t.name.as_str(), Arc::new(mem_table))
            .map_err(|e| SqlValidatorError::SchemaSetup(e.to_string()))?;
    }
    Ok(())
}

/// Recursively walk a LogicalPlan looking for any Join node. JOINs are a
/// hard structural rule violation regardless of on_error - never
/// downgradable to a warning.
fn contains_join(plan: &LogicalPlan) -> bool {
    if matches!(plan, LogicalPlan::Join(_)) {
        return true;
    }
    plan.inputs().iter().any(|child| contains_join(child))
}

/// Heuristic classification of a DataFusion planning error: does the error
/// text indicate an unresolved column reference (candidate for
/// missing_column_mode downgrade) versus a genuine syntax error?
///
/// NOTE: this matches on error message substrings rather than a specific
/// DataFusionError variant, because the exact enum shape has moved between
/// DataFusion versions. This is a known fragility - once the DataFusion
/// version is pinned (see Cargo.toml note), tighten this to match on the
/// concrete error variant instead of string sniffing.
fn classify_plan_error(message: &str) -> Category {
    let lower = message.to_lowercase();
    if lower.contains("no field named") || lower.contains("schema error") {
        Category::Schema
    } else {
        Category::Syntax
    }
}

/// Registers the runtime's real UDFs (get_json_object, avro decimal
/// decode, sha2, etc.) via the runtime's UDF crate, so validation plans
/// SQL against the identical function surface the runtime engine
/// exposes - not a hand-rolled approximation of it.
///
/// `register_local_udfs` takes an optional salt (only consumed by the
/// `crypto` feature's salted-hash UDF, which isn't enabled by the UDF
/// crate's default features) - `None` here since validation never
/// executes these functions anyway, only plans against them.
fn register_runtime_udfs(ctx: &SessionContext) {
    ssync_udf::register_local_udfs(ctx, None).expect("registering runtime UDFs should never fail");
}

/// Detects `SELECT *` / `SELECT t.*` via the raw sqlparser AST, BEFORE
/// DataFusion planning - wildcards get expanded into individual named
/// columns during planning and are indistinguishable from an explicit list
/// afterward, so this check cannot happen post-plan the way the JOIN check
/// does. Also checks inside WITH-clause CTE bodies, not just the outermost
/// query - real transforms use CTEs for BINARY->VARCHAR staging (per the
/// base_prep.sql pattern), so a wildcard hidden inside a CTE needs to be
/// caught too. If sqlparser itself fails to parse the SQL, this returns
/// false and lets DataFusion's own planner surface the real syntax error
/// instead - don't want a parser disagreement between sqlparser and
/// DataFusion's internal parser to produce a misleading "wildcard" finding.
///
/// API RISK (unverified): `SelectItem::Wildcard`/`QualifiedWildcard` tuple
/// shapes, and the `Query.with.cte_tables` traversal, are written against my
/// best understanding of sqlparser 0.62's AST; exact inner types may differ.
fn contains_wildcard_select(sql: &str) -> bool {
    use sqlparser::ast::{Query, SelectItem, SetExpr, Statement};
    use sqlparser::dialect::GenericDialect;
    use sqlparser::parser::Parser;

    fn select_has_wildcard(query: &Query) -> bool {
        if let SetExpr::Select(select) = query.body.as_ref() {
            if select.projection.iter().any(|item| {
                matches!(
                    item,
                    SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
                )
            }) {
                return true;
            }
        }
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                if select_has_wildcard(&cte.query) {
                    return true;
                }
            }
        }
        false
    }

    let Ok(statements) = Parser::parse_sql(&GenericDialect {}, sql) else {
        return false;
    };

    for stmt in statements {
        if let Statement::Query(query) = stmt {
            if select_has_wildcard(&query) {
                return true;
            }
        }
    }
    false
}

pub async fn validate_transform_sql(
    sql: &str,
    upstream_tables: &[UpstreamTable],
    missing_column_mode: Option<MissingColumnMode>,
) -> Result<ValidationResult, SqlValidatorError> {
    if contains_wildcard_select(sql) {
        return Ok(ValidationResult {
            findings: vec![Finding {
                severity: Severity::Error,
                category: Category::Rule,
                message: "SELECT * / qualified wildcards are not permitted - all columns must be explicitly named".to_string(),
            }],
            plan: None,
        });
    }

    let ctx = SessionContext::new();
    register_runtime_udfs(&ctx);
    register_tables(&ctx, upstream_tables)?;

    match ctx.sql(sql).await {
        Ok(df) => {
            let plan = df.logical_plan().clone();

            if contains_join(&plan) {
                return Ok(ValidationResult {
                    findings: vec![Finding {
                        severity: Severity::Error,
                        category: Category::Rule,
                        message: "JOINs are not permitted in pipeline transforms".to_string(),
                    }],
                    plan: None,
                });
            }

            Ok(ValidationResult {
                findings: vec![],
                plan: Some(plan),
            })
        }
        Err(e) => {
            let message = e.to_string();
            let category = classify_plan_error(&message);

            let downgrade = category == Category::Schema
                && matches!(missing_column_mode, Some(MissingColumnMode::NullAndWarn));

            let finding = Finding {
                severity: if downgrade {
                    Severity::Warning
                } else {
                    Severity::Error
                },
                category,
                message,
            };

            Ok(ValidationResult {
                findings: vec![finding],
                plan: None,
            })
        }
    }
}

/// Convenience constructor for the isolated `tests/fixtures/sql_cases/`
/// schema shapes (name/type/nullable JSON triples). Test-only helper.
#[cfg(test)]
pub fn schema_from_field_specs(fields: &[(&str, DataType, bool)]) -> SchemaRef {
    use datafusion::arrow::datatypes::{Field, Schema};
    Arc::new(Schema::new(
        fields
            .iter()
            .map(|(name, dt, nullable)| Field::new(*name, dt.clone(), *nullable))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared_schema() -> SchemaRef {
        schema_from_field_specs(&[
            ("id", DataType::Utf8, false),
            ("customer_id", DataType::Utf8, true),
            ("order_total", DataType::Float64, true),
            ("promo_code", DataType::Utf8, true),
            ("status", DataType::Utf8, true),
            ("source_feed", DataType::Utf8, true),
            (
                "_ssync_ingest_ts",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                true,
            ),
            ("_ssync_record_id", DataType::Utf8, true),
        ])
    }

    fn accounts_schema() -> SchemaRef {
        schema_from_field_specs(&[
            ("order_id", DataType::Utf8, false),
            ("account_id", DataType::Utf8, true),
            ("tier", DataType::Utf8, true),
        ])
    }

    fn upstream() -> Vec<UpstreamTable> {
        vec![
            UpstreamTable {
                name: "prepared".to_string(),
                schema: prepared_schema(),
            },
            UpstreamTable {
                name: "accounts".to_string(),
                schema: accounts_schema(),
            },
        ]
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sql_cases")
    }

    #[tokio::test]
    async fn valid_simple_sql_passes_clean() {
        let sql = std::fs::read_to_string(fixtures_dir().join("valid_simple.sql")).unwrap();
        let result = validate_transform_sql(&sql, &upstream(), None)
            .await
            .unwrap();
        assert!(result.is_valid(), "findings: {:?}", result.findings);
        assert!(result.plan.is_some());
    }

    #[tokio::test]
    async fn wildcard_select_is_hard_error_caught_pre_plan() {
        let sql = std::fs::read_to_string(fixtures_dir().join("wildcard_violation.sql")).unwrap();
        let result =
            validate_transform_sql(&sql, &upstream(), Some(MissingColumnMode::NullAndWarn))
                .await
                .unwrap();
        assert!(!result.is_valid());
        assert_eq!(result.findings[0].category, Category::Rule);
        assert_eq!(result.findings[0].severity, Severity::Error);
        assert!(result.plan.is_none());
    }

    #[tokio::test]
    async fn join_is_hard_error_regardless_of_missing_column_mode() {
        let sql = std::fs::read_to_string(fixtures_dir().join("join_violation.sql")).unwrap();
        let result =
            validate_transform_sql(&sql, &upstream(), Some(MissingColumnMode::NullAndWarn))
                .await
                .unwrap();
        assert!(!result.is_valid());
        assert_eq!(result.findings[0].category, Category::Rule);
        assert_eq!(result.findings[0].severity, Severity::Error);
    }

    #[tokio::test]
    async fn syntax_error_is_hard_error() {
        let sql = std::fs::read_to_string(fixtures_dir().join("syntax_error.sql")).unwrap();
        let result = validate_transform_sql(&sql, &upstream(), None)
            .await
            .unwrap();
        assert!(!result.is_valid());
        assert_eq!(result.findings[0].category, Category::Syntax);
    }

    #[tokio::test]
    async fn missing_column_hard_fails_without_null_and_warn_mode() {
        let sql = std::fs::read_to_string(fixtures_dir().join("missing_column_ref.sql")).unwrap();
        let result = validate_transform_sql(&sql, &upstream(), None)
            .await
            .unwrap();
        assert!(!result.is_valid());
        assert_eq!(result.findings[0].category, Category::Schema);
        assert_eq!(result.findings[0].severity, Severity::Error);
    }

    #[tokio::test]
    async fn missing_column_downgrades_to_warning_under_null_and_warn_mode() {
        let sql = std::fs::read_to_string(fixtures_dir().join("missing_column_ref.sql")).unwrap();
        let result =
            validate_transform_sql(&sql, &upstream(), Some(MissingColumnMode::NullAndWarn))
                .await
                .unwrap();
        // Not a hard failure under this mode.
        assert!(result.is_valid());
        assert_eq!(result.findings[0].severity, Severity::Warning);
        assert_eq!(result.findings[0].category, Category::Schema);
    }

    #[tokio::test]
    async fn untraceable_column_plans_fine_lineage_is_out_of_scope_here() {
        // This module only validates that the SQL plans - it does NOT judge
        // whether output columns trace to a permitted origin. That's
        // lineage_engine's job (Phase 2). Confirms this case is NOT
        // misclassified as a sql_validator error.
        let sql = std::fs::read_to_string(fixtures_dir().join("untraceable_column.sql")).unwrap();
        let result = validate_transform_sql(&sql, &upstream(), None)
            .await
            .unwrap();
        assert!(result.is_valid(), "findings: {:?}", result.findings);
        assert!(result.plan.is_some());
    }
}
