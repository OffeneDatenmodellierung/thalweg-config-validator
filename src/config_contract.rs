//! Canonical in-memory model for a streaming-pipeline config.
//!
//! One struct tree serves YAML, JSON, and TOML. YAML/JSON use camelCase field
//! names (`#[serde(rename_all = "camelCase")]`); TOML uses snake_case, which
//! happens to match Rust's own field naming, so each field additionally
//! carries an explicit `alias` for its raw snake_case form. The one exception
//! that ISN'T pure casing is `subTransforms` (YAML/JSON, plural) vs TOML's
//! `[[sub_transform]]` (singular repeated table) - handled with an alias too,
//! since serde aliases aren't required to agree with the rename_all
//! transform, they just add an accepted alternate name.
//!
//! Fixtures proving all three formats parse to an identical struct live in
//! `tests/fixtures/valid_pipeline.{yaml,json,toml}`.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConfig {
    pub transform: PrimaryTransform,

    #[serde(default)]
    pub dq: Option<DqConfig>,

    #[serde(default)]
    pub sink: Vec<SinkConfig>,

    /// YAML/JSON: `subTransforms` (list). TOML: `[[sub_transform]]` (repeated
    /// table, singular name) - both parse into this one field.
    #[serde(alias = "sub_transform")]
    pub sub_transforms: Vec<SubTransform>,

    #[serde(default)]
    pub batch: Option<BatchConfig>,

    #[serde(default)]
    pub dlq: Option<DlqConfig>,
}

/// The single, primary `[transform]` block. This is a pass-through prep
/// stage - it is NEVER itself a candidate output table, regardless of any
/// sink lane wiring. Only `sub_transforms[].input` may reference it (via its
/// `alias`, conventionally "base").
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryTransform {
    pub alias: String,

    #[serde(alias = "sql_file")]
    pub sql_file: String,

    #[serde(alias = "on_error", default)]
    pub on_error: Option<OnError>,

    #[serde(alias = "missing_column_mode", default)]
    pub missing_column_mode: Option<MissingColumnMode>,

    #[serde(alias = "schema_hint_columns", default)]
    pub schema_hint_columns: Vec<SchemaHintColumn>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissingColumnMode {
    NullAndWarn,
    // Absence of this field entirely is the implicit "hard fail" mode -
    // modelled as `Option<MissingColumnMode> == None` at the call site rather
    // than a variant here, since that's how the real configs express it.
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SchemaHintColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DqConfig {
    pub mode: String,
    #[serde(alias = "drop_rule", default)]
    pub drop_rule: Vec<DropRule>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DropRule {
    pub sql: String,
}

/// Sink lanes are parsed so the config round-trips without error, but per
/// the v1 scope decision, they do NOT feed schema_emitter/ui_model banner
/// logic - engine-per-table resolution (per-backend DDL dialects) is out
/// of scope until explicitly requested.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SinkConfig {
    pub name: String,
    pub receives: String,
    #[serde(rename = "type")]
    pub sink_type: String,
    #[serde(default)]
    pub table: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(alias = "auto_evolve_schema", default)]
    pub auto_evolve_schema: Option<bool>,
    #[serde(alias = "raw_payload_columns", default)]
    pub raw_payload_columns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubTransform {
    pub name: String,
    pub input: String,

    #[serde(alias = "sql_file")]
    pub sql_file: String,

    #[serde(alias = "clean_table", default)]
    pub clean_table: Option<String>,

    #[serde(alias = "quarantine_table", default)]
    pub quarantine_table: Option<String>,

    #[serde(alias = "cluster_by", default)]
    pub cluster_by: Vec<String>,

    /// Simpler per-node backend tag seen in some real configs, coexisting
    /// with the [[sink]] lane-routing model seen in others. Parsed for
    /// completeness; ignored by schema/lineage logic per the v1 sink-scope
    /// decision (same as sink/dq).
    #[serde(alias = "destination_backend", default)]
    pub destination_backend: Option<String>,

    #[serde(alias = "json_expand_columns", default)]
    pub json_expand_columns: Vec<JsonExpandColumn>,
}

impl SubTransform {
    /// A node is virtual iff it has no clean_table (and no quarantine_table -
    /// nothing physically persists it) AND at least one downstream node
    /// references it as `input`. That second half can only be evaluated with
    /// the full sub_transforms list in hand, so this is a cheap necessary-
    /// but-not-sufficient check; see `lineage_engine` for the real
    /// classification once the DAG is built.
    pub fn has_no_sink_target(&self) -> bool {
        self.clean_table.is_none() && self.quarantine_table.is_none()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JsonExpandColumn {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchConfig {
    #[serde(alias = "max_wait_ms")]
    pub max_wait_ms: u64,
    #[serde(alias = "max_records")]
    pub max_records: u64,
    #[serde(alias = "max_bytes")]
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DlqConfig {
    pub enabled: bool,
    #[serde(alias = "schema_name", default)]
    pub schema_name: Option<String>,
    #[serde(alias = "table_name")]
    pub table_name: String,
}

impl PipelineConfig {
    /// Build a name -> SubTransform lookup. Used by lineage_engine for DAG
    /// construction from `input` edges (topological, not declaration order -
    /// see ADR note in tests/fixtures/valid_pipeline.toml comments).
    pub fn sub_transform_index(&self) -> HashMap<&str, &SubTransform> {
        self.sub_transforms
            .iter()
            .map(|t| (t.name.as_str(), t))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn yaml_json_toml_parse_to_identical_struct() {
        let yaml_raw = std::fs::read_to_string(fixtures_dir().join("valid_pipeline.yaml"))
            .expect("read yaml fixture");
        let json_raw = std::fs::read_to_string(fixtures_dir().join("valid_pipeline.json"))
            .expect("read json fixture");
        let toml_raw = std::fs::read_to_string(fixtures_dir().join("valid_pipeline.toml"))
            .expect("read toml fixture");

        let from_yaml: PipelineConfig =
            serde_yaml::from_str(&yaml_raw).expect("parse yaml fixture");
        let from_json: PipelineConfig =
            serde_json::from_str(&json_raw).expect("parse json fixture");
        let from_toml: PipelineConfig = toml::from_str(&toml_raw).expect("parse toml fixture");

        assert_eq!(from_yaml, from_json, "YAML and JSON must parse identically");
        assert_eq!(from_yaml, from_toml, "YAML and TOML must parse identically");
    }

    #[test]
    fn primary_transform_is_never_a_sub_transform_input_target_by_name_collision() {
        // Guards against a future config accidentally naming a sub_transform
        // the same as the primary transform's alias, which would make the
        // "primary transform is never persisted" rule ambiguous downstream.
        let yaml_raw = std::fs::read_to_string(fixtures_dir().join("valid_pipeline.yaml"))
            .expect("read yaml fixture");
        let config: PipelineConfig = serde_yaml::from_str(&yaml_raw).expect("parse yaml fixture");

        assert!(config
            .sub_transforms
            .iter()
            .all(|t| t.name != config.transform.alias));
    }
}
