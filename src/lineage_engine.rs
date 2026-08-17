//! DAG construction over sub_transforms, topological ordering, per-node SQL
//! validation via sql_validator, and column-origin (lineage) classification.
//!
//! KNOWN API RISK (not verified by compiling): `arrow_schema_from_plan` uses
//! `plan.schema().inner().clone()` to get from DataFusion's `DFSchemaRef` to
//! an arrow `SchemaRef`. If that method doesn't exist on your DFSchema,
//! likely alternatives are `.as_arrow()` or building a `Schema` manually
//! from `plan.schema().fields()`. Similarly, `collect_column_refs` is a
//! manual recursive walker over a subset of `Expr` variants (Column, Alias,
//! Cast/TryCast, BinaryExpr, ScalarFunction, Not/IsNull/IsNotNull/Negative,
//! Literal) rather than a call to DataFusion's own `expr_to_columns` utility
//! (avoided because I couldn't confirm its exact import path for 54.1.0).
//! If a real transform uses an expression shape not listed here (CASE,
//! window functions, aggregates), extend the match arms - it currently
//! degrades safely (under-reports refs) rather than panicking.
//!
//! No `SELECT *` is permitted anywhere (enforced in sql_validator, checked
//! pre-plan via sqlparser since wildcards vanish once DataFusion expands
//! them). That means `permitted_origins` below only needs to check a
//! node's actual registered upstream schema - no blanket seed/hint
//! fallback for wildcard-passthrough semantics, since none exist.

use crate::config_contract::{PipelineConfig, PrimaryTransform, SubTransform};
use crate::seed_registry;
use crate::sql_validator::{parse_arrow_type, validate_transform_sql, Finding, UpstreamTable};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::logical_expr::{Expr, LogicalPlan};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Real configs express `sqlFile` as an absolute container-mount path (e.g.
/// `/transforms/gpd_base_prep.sql`, matching a ConfigMap mount at
/// `/transforms`), not a path relative to any host directory. `Path::join`
/// treats an absolute second argument as replacing the base entirely rather
/// than appending to it - so joining `--transforms-dir` with an absolute
/// `sqlFile` silently discards `--transforms-dir` and looks for the literal
/// container path on the host, which doesn't exist.
///
/// ASSUMPTION: the transforms directory is flat (no subfolders) - this
/// takes only the file's basename when `sql_file` is absolute. If a real
/// config ever nests further (e.g. `/transforms/racing/foo.sql`), this
/// needs to preserve path components after the mount root instead of
/// collapsing to the basename - hasn't been seen in configs so far, so not
/// implemented speculatively.
fn resolve_sql_path(transforms_dir: &Path, sql_file: &str) -> PathBuf {
    let sql_path = Path::new(sql_file);
    if sql_path.is_absolute() {
        if let Some(name) = sql_path.file_name() {
            return transforms_dir.join(name);
        }
    }
    transforms_dir.join(sql_path)
}

#[derive(Debug, Error)]
pub enum LineageEngineError {
    #[error("sub_transform '{0}' has input '{1}' which does not resolve to the primary transform or any other sub_transform")]
    UnknownInputReference(String, String),

    #[error("cycle detected in sub_transform DAG: {0:?}")]
    CycleDetected(Vec<String>),

    #[error("failed to read SQL file {path}: {source}")]
    ReadSql {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("sql_validator setup failed for '{0}': {1}")]
    Validator(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner {
    Green,
    Red,
    Virtual,
}

#[derive(Debug, Clone)]
pub struct ColumnOrigin {
    pub name: String,
    pub traceable: bool,
}

#[derive(Debug)]
pub struct LineageNode {
    pub name: String,
    pub is_primary: bool,
    pub input: Option<String>,
    pub clean_table: Option<String>,
    pub quarantine_table: Option<String>,
    pub is_virtual: bool,
    pub banner: Banner,
    pub output_schema: Option<SchemaRef>,
    pub column_origins: Vec<ColumnOrigin>,
    pub findings: Vec<Finding>,
}

/// Builds the upstream schema for the primary `[transform]` block: every
/// seed_registry column, plus any schema_hint_columns declared on it
/// (nullable, since hint columns exist precisely because they're not always
/// present upstream).
pub fn primary_transform_upstream_schema(primary: &PrimaryTransform) -> SchemaRef {
    let mut fields: Vec<Field> = seed_registry::SEED_COLUMNS
        .iter()
        .map(|c| Field::new(c.name, parse_arrow_type(c.arrow_type), true))
        .collect();

    for hint in &primary.schema_hint_columns {
        fields.push(Field::new(
            hint.name.as_str(),
            parse_arrow_type(&hint.column_type),
            true,
        ));
    }

    Arc::new(Schema::new(fields))
}

fn arrow_schema_from_plan(plan: &LogicalPlan) -> SchemaRef {
    plan.schema().inner().clone()
}

fn collect_column_refs(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Column(c) => {
            out.insert(c.name.clone());
        }
        Expr::Alias(alias) => collect_column_refs(&alias.expr, out),
        Expr::Cast(c) => collect_column_refs(&c.expr, out),
        Expr::TryCast(c) => collect_column_refs(&c.expr, out),
        Expr::BinaryExpr(b) => {
            collect_column_refs(&b.left, out);
            collect_column_refs(&b.right, out);
        }
        Expr::Not(e) | Expr::IsNull(e) | Expr::IsNotNull(e) | Expr::Negative(e) => {
            collect_column_refs(e, out)
        }
        Expr::ScalarFunction(f) => {
            for arg in &f.args {
                collect_column_refs(arg, out);
            }
        }
        Expr::Literal(..) => {}
        _ => {
            // Conservative fallback - see module doc comment.
        }
    }
}

fn synthetic_expand_fields(node: &SubTransform) -> Vec<Field> {
    let mut out = Vec::new();
    for expand in &node.json_expand_columns {
        for field_name in &expand.fields {
            out.push(Field::new(
                format!("{}_{}", expand.name, field_name),
                DataType::Utf8,
                true,
            ));
        }
    }
    out
}

fn schema_union(base: &Schema, extra: &[Field]) -> SchemaRef {
    let mut fields: Vec<Field> = base.fields().iter().map(|f| f.as_ref().clone()).collect();
    let existing: HashSet<String> = fields.iter().map(|f| f.name().clone()).collect();
    for f in extra {
        if !existing.contains(f.name()) {
            fields.push(f.clone());
        }
    }
    Arc::new(Schema::new(fields))
}

/// Topologically sorts sub_transforms by `input` edges (NOT declaration
/// order - the real engine resolves execution order at startup, per the
/// comment in the production TOML config). Returns an error on an unknown
/// input reference or a cycle.
fn topological_order<'a>(
    primary_alias: &str,
    sub_transforms: &'a [SubTransform],
    by_name: &HashMap<&'a str, &'a SubTransform>,
) -> Result<Vec<&'a SubTransform>, LineageEngineError> {
    for t in sub_transforms {
        if t.input != primary_alias && !by_name.contains_key(t.input.as_str()) {
            return Err(LineageEngineError::UnknownInputReference(
                t.name.clone(),
                t.input.clone(),
            ));
        }
    }

    let mut order = Vec::with_capacity(sub_transforms.len());
    let mut visited: HashSet<&str> = HashSet::new();
    let mut in_progress: HashSet<&str> = HashSet::new();

    fn visit<'a>(
        name: &'a str,
        primary_alias: &str,
        by_name: &HashMap<&'a str, &'a SubTransform>,
        visited: &mut HashSet<&'a str>,
        in_progress: &mut HashSet<&'a str>,
        order: &mut Vec<&'a SubTransform>,
        stack: &mut Vec<String>,
    ) -> Result<(), LineageEngineError> {
        if visited.contains(name) {
            return Ok(());
        }
        if in_progress.contains(name) {
            stack.push(name.to_string());
            return Err(LineageEngineError::CycleDetected(stack.clone()));
        }
        let Some(node) = by_name.get(name) else {
            // Reached the primary transform - nothing further upstream.
            return Ok(());
        };
        in_progress.insert(name);
        stack.push(name.to_string());

        if node.input != primary_alias {
            visit(
                node.input.as_str(),
                primary_alias,
                by_name,
                visited,
                in_progress,
                order,
                stack,
            )?;
        }

        stack.pop();
        in_progress.remove(name);
        visited.insert(name);
        order.push(node);
        Ok(())
    }

    for t in sub_transforms {
        let mut stack = Vec::new();
        visit(
            t.name.as_str(),
            primary_alias,
            by_name,
            &mut visited,
            &mut in_progress,
            &mut order,
            &mut stack,
        )?;
    }

    Ok(order)
}

pub async fn build_lineage(
    config: &PipelineConfig,
    transforms_dir: &Path,
) -> Result<Vec<LineageNode>, LineageEngineError> {
    let primary_alias = config.transform.alias.clone();

    // --- Primary transform: always virtual, never a candidate output table ---
    let source_schema = primary_transform_upstream_schema(&config.transform);
    let primary_sql_path = resolve_sql_path(transforms_dir, &config.transform.sql_file);
    let primary_sql =
        std::fs::read_to_string(&primary_sql_path).map_err(|e| LineageEngineError::ReadSql {
            path: primary_sql_path.display().to_string(),
            source: e,
        })?;

    let primary_validation = validate_transform_sql(
        &primary_sql,
        &[UpstreamTable {
            name: "source".to_string(),
            schema: source_schema,
        }],
        config.transform.missing_column_mode,
    )
    .await
    .map_err(|e| LineageEngineError::Validator(primary_alias.clone(), e.to_string()))?;

    let primary_output_schema = primary_validation.plan.as_ref().map(arrow_schema_from_plan);

    let mut node_by_name: HashMap<String, SchemaRef> = HashMap::new();
    if let Some(schema) = &primary_output_schema {
        node_by_name.insert(primary_alias.clone(), schema.clone());
    }

    let mut nodes = vec![LineageNode {
        name: primary_alias.clone(),
        is_primary: true,
        input: None,
        clean_table: None,
        quarantine_table: None,
        is_virtual: true,
        banner: if primary_validation.is_valid() {
            Banner::Virtual
        } else {
            Banner::Red
        },
        output_schema: primary_output_schema,
        column_origins: vec![],
        findings: primary_validation.findings,
    }];

    // --- Sub-transforms, topologically ordered ---
    let by_name = config.sub_transform_index();
    let ordered = topological_order(&primary_alias, &config.sub_transforms, &by_name)?;

    let referenced_as_input: HashSet<&str> = config
        .sub_transforms
        .iter()
        .map(|t| t.input.as_str())
        .collect();

    for node in ordered {
        let upstream_schema_opt = node_by_name.get(node.input.as_str()).cloned();

        let (validation, column_origins, output_schema);

        if let Some(upstream_schema) = upstream_schema_opt {
            let synthetic = synthetic_expand_fields(node);
            let registered_schema = schema_union(&upstream_schema, &synthetic);

            // No SELECT * is permitted (enforced in sql_validator), so a
            // column's only legitimate origins are whatever this node's
            // actual registered upstream schema exposes - no blanket
            // seed/hint fallback needed or wanted, since without wildcard
            // passthrough there's no legitimate way for a column to reach
            // this node except by being explicitly present in
            // registered_schema already.
            let permitted_origins: HashSet<String> = registered_schema
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();

            let sql_path = resolve_sql_path(transforms_dir, &node.sql_file);
            let sql =
                std::fs::read_to_string(&sql_path).map_err(|e| LineageEngineError::ReadSql {
                    path: sql_path.display().to_string(),
                    source: e,
                })?;

            let v = validate_transform_sql(
                &sql,
                &[UpstreamTable {
                    // Every real transform SQL file references its input
                    // generically as `FROM source` - the engine binds
                    // whatever `input` the config declares to that literal
                    // name at runtime; the SQL text never names the input
                    // node directly ("base", "prepared", "markets_virtual",
                    // ...). Confirmed by a real run: every downstream
                    // node's planning error named the table "source",
                    // never its own input's name.
                    name: "source".to_string(),
                    schema: registered_schema,
                }],
                None, // sub_transforms carry no missing_column_mode - always hard-fail
            )
            .await
            .map_err(|e| LineageEngineError::Validator(node.name.clone(), e.to_string()))?;

            let mut origins = Vec::new();
            if let Some(LogicalPlan::Projection(proj)) = &v.plan {
                for (idx, expr) in proj.expr.iter().enumerate() {
                    let name = proj.schema.field(idx).name().clone();
                    let mut refs = HashSet::new();
                    collect_column_refs(expr, &mut refs);
                    let traceable =
                        !refs.is_empty() && refs.iter().all(|r| permitted_origins.contains(r));
                    origins.push(ColumnOrigin { name, traceable });
                }
            }
            // If the plan's top node isn't a bare Projection, origins stays
            // empty rather than guessing - worth revisiting if a real
            // transform's plan shape hits this.

            // Downstream-visible schema = this node's actual literal SQL
            // output UNION its own synthetic expand fields. Both
            // items_virtual->items (expansion declared by the parent,
            // consumed by a different downstream node) and accounts
            // (expansion declared and consumed by the same node) need this
            // union to propagate correctly - a node's own synthetic fields
            // must be visible to its children even when its own SQL never
            // selects them.
            output_schema = v
                .plan
                .as_ref()
                .map(arrow_schema_from_plan)
                .map(|s| schema_union(&s, &synthetic));
            validation = v;
            column_origins = origins;
        } else {
            // This node's input never produced a schema (its own
            // validation failed, or it in turn was blocked by something
            // further upstream). Validating this node's own SQL against an
            // empty/fabricated schema would only produce a confusing,
            // arbitrary "no field named X" error that isn't actually about
            // this node's SQL quality - skip it and point straight at the
            // real root cause instead.
            validation = crate::sql_validator::ValidationResult {
                findings: vec![Finding {
                    severity: crate::sql_validator::Severity::Error,
                    category: crate::sql_validator::Category::Schema,
                    message: format!(
                        "Blocked: upstream input '{}' failed to validate, so this table's schema and column lineage cannot be determined until that is fixed",
                        node.input
                    ),
                }],
                plan: None,
            };
            column_origins = vec![];
            output_schema = None;
        }

        if let Some(schema) = &output_schema {
            node_by_name.insert(node.name.clone(), schema.clone());
        }

        let has_downstream_consumer = referenced_as_input.contains(node.name.as_str());
        let is_virtual = node.has_no_sink_target() && has_downstream_consumer;

        let has_error = !validation.is_valid();
        let has_untraceable = column_origins.iter().any(|c| !c.traceable);

        let banner = if has_error || has_untraceable {
            Banner::Red
        } else if is_virtual {
            Banner::Virtual
        } else {
            Banner::Green
        };

        nodes.push(LineageNode {
            name: node.name.clone(),
            is_primary: false,
            input: Some(node.input.clone()),
            clean_table: node.clean_table.clone(),
            quarantine_table: node.quarantine_table.clone(),
            is_virtual,
            banner,
            output_schema,
            column_origins,
            findings: validation.findings,
        });
    }

    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_format;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn resolve_sql_path_uses_basename_for_absolute_container_paths() {
        let dir = Path::new("/host/transforms");
        assert_eq!(
            resolve_sql_path(dir, "/transforms/gpd_base_prep.sql"),
            Path::new("/host/transforms/gpd_base_prep.sql")
        );
    }

    #[test]
    fn resolve_sql_path_joins_relative_paths_normally() {
        let dir = Path::new("/host/transforms");
        assert_eq!(
            resolve_sql_path(dir, "gpd_base_prep.sql"),
            Path::new("/host/transforms/gpd_base_prep.sql")
        );
    }

    #[tokio::test]
    async fn valid_pipeline_all_nodes_classify_correctly() {
        let config =
            config_format::load(&fixtures_dir().join("valid_pipeline.toml"), None).unwrap();
        let nodes = build_lineage(&config, &fixtures_dir()).await.unwrap();

        let base = nodes.iter().find(|n| n.is_primary).unwrap();
        assert_eq!(
            base.banner,
            Banner::Virtual,
            "findings: {:?}",
            base.findings
        );

        for virtual_name in ["prepared", "items_virtual"] {
            let n = nodes.iter().find(|n| n.name == virtual_name).unwrap();
            assert!(n.is_virtual, "{virtual_name} should be virtual");
            assert_eq!(
                n.banner,
                Banner::Virtual,
                "{virtual_name} findings: {:?}",
                n.findings
            );
        }

        for leaf_name in ["orders", "order_status", "items", "accounts", "extras"] {
            let n = nodes.iter().find(|n| n.name == leaf_name).unwrap();
            assert!(!n.is_virtual, "{leaf_name} should not be virtual");
            assert_eq!(
                n.banner,
                Banner::Green,
                "{leaf_name} findings: {:?}",
                n.findings
            );
        }
    }

    #[tokio::test]
    async fn declaration_order_does_not_matter_for_topological_sort() {
        let mut config =
            config_format::load(&fixtures_dir().join("valid_pipeline.toml"), None).unwrap();
        config.sub_transforms.reverse();

        let nodes = build_lineage(&config, &fixtures_dir()).await.unwrap();
        let items = nodes.iter().find(|n| n.name == "items").unwrap();
        assert_eq!(
            items.banner,
            Banner::Green,
            "findings: {:?}",
            items.findings
        );
    }

    #[tokio::test]
    async fn downstream_of_a_failed_node_gets_a_clear_blocked_finding_not_an_arbitrary_schema_error(
    ) {
        let mut config =
            config_format::load(&fixtures_dir().join("valid_pipeline.toml"), None).unwrap();

        // Break "prepared" by pointing it at SQL that references a column
        // absent from its actual upstream (base's real output) - this
        // fixture happens to reference `loyalty_tier`, which doesn't exist
        // in this pipeline's schema at all.
        let prepared = config
            .sub_transforms
            .iter_mut()
            .find(|t| t.name == "prepared")
            .unwrap();
        prepared.sql_file = "sql_cases/missing_column_ref.sql".to_string();

        let nodes = build_lineage(&config, &fixtures_dir()).await.unwrap();

        let prepared_node = nodes.iter().find(|n| n.name == "prepared").unwrap();
        assert_eq!(prepared_node.banner, Banner::Red);

        // Anything downstream of "prepared" should get ONE clear
        // "blocked by upstream" finding, not an arbitrary schema error
        // about its own SQL.
        for downstream_name in [
            "orders",
            "order_status",
            "items_virtual",
            "accounts",
            "extras",
        ] {
            let n = nodes.iter().find(|n| n.name == downstream_name).unwrap();
            assert_eq!(n.banner, Banner::Red, "{downstream_name}");
            assert_eq!(
                n.findings.len(),
                1,
                "{downstream_name} findings: {:?}",
                n.findings
            );
            assert!(
                n.findings[0].message.contains("Blocked"),
                "{downstream_name} finding: {:?}",
                n.findings[0]
            );
        }
    }

    #[tokio::test]
    async fn unknown_input_reference_is_an_error() {
        let mut config =
            config_format::load(&fixtures_dir().join("valid_pipeline.toml"), None).unwrap();
        config.sub_transforms[0].input = "does_not_exist".to_string();

        let result = build_lineage(&config, &fixtures_dir()).await;
        assert!(matches!(
            result,
            Err(LineageEngineError::UnknownInputReference(_, _))
        ));
    }
}
