//! Detects config format from file extension (with an override for cases
//! where the extension isn't reliable), parses into a common
//! `serde_json::Value` tree regardless of source format, NORMALIZES known
//! shape variance in that tree, then deserializes the normalized value into
//! the single `PipelineConfig` model in `config_contract`.
//!
//! Real configs have been observed in at least three different top-level
//! transform-block shapes:
//!   1. flat: top-level `transform` + top-level `subTransforms`
//!   2. TOML-native: top-level `[transform]` + repeated `[[sub_transform]]`
//!      (handled by config_contract's own field aliasing, not here)
//!   3. wrapped: top-level `transforms: { base: {...}, subTransforms: [...] }`
//!
//! `normalize_transform_shape` hoists shape 3's nested `base`/`subTransforms`
//! up to the top level so config_contract only ever has to handle shape 1.
//! Given this variance has already surfaced three times, treat this as a
//! likely-still-incomplete list - if a fourth shape turns up, extend the
//! normalizer rather than config_contract's struct definitions directly.
//!
//! API RISK (unverified): parsing directly into `serde_json::Value` from a
//! `serde_yaml`/`toml` source (rather than each crate's own native `Value`
//! type first) relies on their Deserializer implementations supporting
//! `deserialize_any` well enough for serde_json::Value's generic capture.
//! This is standard practice across the serde ecosystem and I'd expect it
//! to work, but haven't compiled it.

use crate::config_contract::PipelineConfig;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Yaml,
    Json,
    Toml,
}

impl ConfigFormat {
    pub fn from_extension(path: &Path) -> Result<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") => Ok(ConfigFormat::Yaml),
            Some("json") => Ok(ConfigFormat::Json),
            Some("toml") => Ok(ConfigFormat::Toml),
            other => bail!(
                "cannot infer config format from extension {:?} for {}; pass --format explicitly",
                other,
                path.display()
            ),
        }
    }
}

/// Hoists `transforms.base` -> top-level `transform` and
/// `transforms.subTransforms` (or `transforms.sub_transform`) -> top-level
/// `subTransforms`, if a `transforms` wrapper object is present. A no-op if
/// the config already uses the flat shape (no `transforms` key, or one
/// without `base`/`subTransforms` inside it) - existing flat-shape fixtures
/// are unaffected. Doesn't remove the original `transforms` key; leftover
/// unrecognised keys are silently ignored by PipelineConfig's deserializer
/// (no `deny_unknown_fields` anywhere in the contract).
fn normalize_transform_shape(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };

    let Some(transforms_obj) = obj.get("transforms").and_then(|v| v.as_object()).cloned() else {
        return value;
    };

    if let Some(base) = transforms_obj.get("base") {
        obj.entry("transform").or_insert_with(|| base.clone());
    }

    if let Some(sub) = transforms_obj
        .get("subTransforms")
        .or_else(|| transforms_obj.get("sub_transform"))
    {
        obj.entry("subTransforms").or_insert_with(|| sub.clone());
    }

    value
}

pub fn load(path: &Path, format_override: Option<ConfigFormat>) -> Result<PipelineConfig> {
    let format = match format_override {
        Some(f) => f,
        None => ConfigFormat::from_extension(path)?,
    };

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;

    let raw_value: Value = match format {
        ConfigFormat::Yaml => serde_yaml::from_str(&raw).context("parsing YAML config")?,
        ConfigFormat::Json => serde_json::from_str(&raw).context("parsing JSON config")?,
        ConfigFormat::Toml => toml::from_str(&raw).context("parsing TOML config")?,
    };

    let normalized = normalize_transform_shape(raw_value);

    let config: PipelineConfig =
        serde_json::from_value(normalized).context("mapping parsed config to contract")?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format_from_extension() {
        assert_eq!(
            ConfigFormat::from_extension(Path::new("x.yaml")).unwrap(),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigFormat::from_extension(Path::new("x.yml")).unwrap(),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigFormat::from_extension(Path::new("x.json")).unwrap(),
            ConfigFormat::Json
        );
        assert_eq!(
            ConfigFormat::from_extension(Path::new("x.toml")).unwrap(),
            ConfigFormat::Toml
        );
        assert!(ConfigFormat::from_extension(Path::new("x.conf")).is_err());
    }

    #[test]
    fn all_three_fixture_formats_load_to_identical_config() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let yaml = load(&dir.join("valid_pipeline.yaml"), None).unwrap();
        let json = load(&dir.join("valid_pipeline.json"), None).unwrap();
        let toml = load(&dir.join("valid_pipeline.toml"), None).unwrap();
        assert_eq!(yaml, json);
        assert_eq!(yaml, toml);
    }

    #[test]
    fn wrapped_transforms_shape_normalizes_to_the_same_config_as_flat_shape() {
        let flat = serde_json::json!({
            "transform": {
                "alias": "base",
                "sqlFile": "x.sql"
            },
            "subTransforms": [
                { "name": "prepared", "input": "base", "sqlFile": "y.sql" }
            ]
        });

        let wrapped = serde_json::json!({
            "transforms": {
                "base": {
                    "alias": "base",
                    "sqlFile": "x.sql"
                },
                "subTransforms": [
                    { "name": "prepared", "input": "base", "sqlFile": "y.sql" }
                ]
            }
        });

        let flat_config: PipelineConfig = serde_json::from_value(normalize_transform_shape(flat)).unwrap();
        let wrapped_config: PipelineConfig =
            serde_json::from_value(normalize_transform_shape(wrapped)).unwrap();

        assert_eq!(flat_config, wrapped_config);
    }
}
