//! Config-level lints — invariants that live above any single transform's
//! SQL and cross the sub_transforms × batch boundary. Distinct from
//! `sql_validator` (per-SQL-file structural rules) and `lineage_engine`
//! (per-node DAG classification): these findings are attached to the
//! pipeline as a whole, not to any one table.
//!
//! Each lint is a pure function of the parsed [`PipelineConfig`] — no I/O,
//! no DataFusion planning. Findings are surfaced as
//! [`PipelineFinding`]s and collected into [`PipelineLintReport`], which
//! `reporter` renders in a separate section above the per-table blocks.
//!
//! Adding a new lint: implement the check as a private function that
//! pushes to `&mut Vec<PipelineFinding>`, then call it from
//! [`run_pipeline_lints`]. Keep each lint deliberately narrow and named
//! after the specific failure mode it defends against — a
//! `LintId::ArrowI32FanoutRisk` is more useful than a generic
//! `LintId::BatchSizingSuspicious` because operators can grep the
//! framework/incident write-ups by that ID.

use crate::config_contract::{BatchConfig, PipelineConfig, SubTransform};
use serde::Serialize;

/// Stable string ID for each lint, so downstream tooling (CI grep, JSON
/// filters) can suppress or escalate on a specific class without matching
/// against message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintId {
    /// `batch.maxRecords` set high enough that a batch of
    /// upstream-compliant records can build a post-`jsonExpandColumns`
    /// Arrow `StringArray` that exceeds the i32 offset ceiling
    /// (`i32::MAX` ≈ 2.147 GB), causing an unrecoverable OOM death
    /// spiral in the downstream streaming runtime.
    ArrowI32FanoutRisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineFinding {
    pub id: LintId,
    pub severity: PipelineSeverity,
    pub message: String,
    /// Names of the `sub_transforms[]` involved in the finding (typically
    /// the nodes carrying `jsonExpandColumns`). Empty when the lint is
    /// scoped to top-level config only.
    pub related_nodes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PipelineLintReport {
    pub findings: Vec<PipelineFinding>,
}

impl PipelineLintReport {
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn has_error(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == PipelineSeverity::Error)
    }
}

// ---------------------------------------------------------------------------
// Arrow i32 fanout-overflow lint
// ---------------------------------------------------------------------------

/// Fanout arithmetic for the JSON-explode overflow class.
///
/// Assumes an upstream publisher contract that caps the size of nested
/// arrays ("selections per record", using the terminology of the market
/// this validator was first authored against — substitute your own domain
/// noun freely). Let:
///
/// * `S` = documented maximum selections per record for a compliant
///   upstream publisher.
/// * `B` = uncompressed byte size of a single selection.
///
/// Then the post-explode Utf8 column bytes for a batch of `N` records is
/// approximately:
///
/// ```text
///   N × S × B
/// ```
///
/// which reaches Arrow's i32 offset ceiling (`i32::MAX` ≈ 2^31 - 1) at
/// `N > 2^31 / (S × B)`. With the defaults below (`S = 6400`, `B = 285`)
/// this is **1177 records** — anything above that can overflow under
/// fully-compliant upstream traffic, with no downstream mitigation
/// available at config-review time.
///
/// The constants below reflect one real-world upstream contract; adjust
/// them (and re-run the tests) if your pipeline's publishers document a
/// different ceiling. The derivation itself is invariant.
pub struct ArrowI32Overflow;

impl ArrowI32Overflow {
    /// Documented maximum selections per record under the upstream
    /// publisher's "safe" band — i.e. the largest fanout a compliant
    /// publisher will emit. Used directly to derive the operative
    /// overflow threshold below.
    pub const UPSTREAM_SAFE_SELECTIONS_PER_RECORD: u64 = 6_400;

    /// Uncompressed byte size of a single selection, per the upstream
    /// publisher guidelines used to derive the threshold.
    pub const SELECTION_BYTES: u64 = 285;

    /// Arrow's i32 offset ceiling for `Utf8` `StringArray`. Anything at or
    /// above this triggers `Arrow error: Offset overflow`.
    pub const I32_STRINGARRAY_CEILING: u64 = i32::MAX as u64;

    /// The batch-size threshold above which a batch of upstream-*safe*
    /// records — i.e. traffic that respects the upstream guideline in
    /// full — can still overflow the exploded column. This is the
    /// operative warning threshold: batches larger than this can OOM the
    /// downstream runtime without any upstream contract violation, and no
    /// benign traffic mix saves them.
    ///
    /// A hypothetical "escalate to Error at some larger N" would need a
    /// second cliff in the arithmetic to be honest — there isn't one.
    /// Overflow risk is monotonic in N once N > this constant. Keep the
    /// lint as a single Warning tier rather than inventing a fake tier
    /// boundary.
    pub const SAFE_OVERFLOW_THRESHOLD_RECORDS: u64 = Self::I32_STRINGARRAY_CEILING
        / (Self::UPSTREAM_SAFE_SELECTIONS_PER_RECORD * Self::SELECTION_BYTES);
}

/// True if any sub_transform declares a `jsonExpandColumns` block —
/// i.e. this pipeline explodes JSON arrays into rows and is therefore
/// exposed to the fanout amplification pattern that drives the i32
/// overflow. Pipelines with no explode nodes cannot hit this class at all,
/// so the lint is skipped for them.
fn has_json_explode(sub_transforms: &[SubTransform]) -> bool {
    sub_transforms
        .iter()
        .any(|t| !t.json_expand_columns.is_empty())
}

fn explode_nodes(sub_transforms: &[SubTransform]) -> Vec<String> {
    sub_transforms
        .iter()
        .filter(|t| !t.json_expand_columns.is_empty())
        .map(|t| t.name.clone())
        .collect()
}

fn check_arrow_i32_fanout_risk(
    batch: &BatchConfig,
    sub_transforms: &[SubTransform],
    out: &mut Vec<PipelineFinding>,
) {
    if !has_json_explode(sub_transforms) {
        return;
    }

    let n = batch.max_records;
    let safe = ArrowI32Overflow::SAFE_OVERFLOW_THRESHOLD_RECORDS;

    if n <= safe {
        return;
    }

    let related = explode_nodes(sub_transforms);
    let sel_safe = ArrowI32Overflow::UPSTREAM_SAFE_SELECTIONS_PER_RECORD;

    let message = format!(
        "batch.maxRecords={n} is above the {safe}-record threshold at which a batch of \
         upstream-compliant records can overflow Arrow's i32 offset ceiling \
         post-jsonExpandColumns. \
         \n\
         Arithmetic: post-explode column bytes = input_rows × selections_per_record × 285 B. \
         At the upstream-safe ceiling of {sel_safe} selections/record, overflow starts at \
         N > 2^31 / ({sel_safe} × 285) = {safe}. \
         Underlying runtime issue: the downstream runtime's JSON-array explode \
         implementation emits Utf8 arrays with i32 offsets; the durable fix is \
         LargeStringArray/i64 offsets or post-explode chunking. Recommended cap \
         until that ships: maxRecords ≤ {safe}. \
         Explode nodes: [{nodes}].",
        nodes = related.join(", "),
    );

    out.push(PipelineFinding {
        id: LintId::ArrowI32FanoutRisk,
        severity: PipelineSeverity::Warning,
        message,
        related_nodes: related,
    });
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run every registered pipeline-level lint against `config`. Order is
/// deterministic (findings appear in the same order across runs) so CI
/// diffs stay stable.
pub fn run_pipeline_lints(config: &PipelineConfig) -> PipelineLintReport {
    let mut findings = Vec::new();

    if let Some(batch) = &config.batch {
        check_arrow_i32_fanout_risk(batch, &config.sub_transforms, &mut findings);
    }

    PipelineLintReport { findings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_contract::{
        BatchConfig, JsonExpandColumn, PipelineConfig, PrimaryTransform, SubTransform,
    };

    fn tf(
        name: &str,
        input: &str,
        clean_table: Option<&str>,
        expands: Vec<JsonExpandColumn>,
    ) -> SubTransform {
        SubTransform {
            name: name.to_string(),
            input: input.to_string(),
            sql_file: format!("{name}.sql"),
            clean_table: clean_table.map(str::to_string),
            quarantine_table: None,
            cluster_by: vec![],
            destination_backend: None,
            json_expand_columns: expands,
        }
    }

    fn base_config(max_records: u64, sub_transforms: Vec<SubTransform>) -> PipelineConfig {
        PipelineConfig {
            transform: PrimaryTransform {
                alias: "base".to_string(),
                sql_file: "base.sql".to_string(),
                on_error: None,
                missing_column_mode: None,
                schema_hint_columns: vec![],
            },
            dq: None,
            sink: vec![],
            sub_transforms,
            batch: Some(BatchConfig {
                max_wait_ms: 100,
                max_records,
                max_bytes: 20_971_520,
            }),
            dlq: None,
        }
    }

    fn expand(name: &str, fields: &[&str]) -> JsonExpandColumn {
        JsonExpandColumn {
            name: name.to_string(),
            fields: fields.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn threshold_arithmetic_matches_documented_derivation() {
        // Guards against silent drift if any of the constants change:
        // update the derivation comment in ArrowI32Overflow together with
        // the constant, and this test proves the two remain consistent.
        assert_eq!(
            ArrowI32Overflow::SAFE_OVERFLOW_THRESHOLD_RECORDS,
            (i32::MAX as u64) / (6_400 * 285)
        );
        // Anchor: a conservative deployed cap (500 records) must sit
        // comfortably below the safe threshold. If someone raises the
        // constants without thinking, this catches it.
        // (`const { assert!(..) }` blocks per clippy — both sides are const,
        // and the compile-time form runs at build time rather than test time.)
        const _: () = assert!(500 < ArrowI32Overflow::SAFE_OVERFLOW_THRESHOLD_RECORDS);
        // Second anchor: a value that has historically triggered the
        // overflow (3000 records) must be ABOVE the threshold.
        const _: () = assert!(3_000 > ArrowI32Overflow::SAFE_OVERFLOW_THRESHOLD_RECORDS);
    }

    #[test]
    fn no_explode_no_finding_even_at_high_max_records() {
        // A pipeline with no jsonExpandColumns cannot hit this class,
        // so the lint should stay silent no matter how large max_records is.
        let config = base_config(
            10_000,
            vec![tf("orders", "base", Some("orders_clean"), vec![])],
        );
        let report = run_pipeline_lints(&config);
        assert!(
            report.is_empty(),
            "unexpected findings: {:?}",
            report.findings
        );
    }

    #[test]
    fn explode_with_conservative_max_records_stays_silent() {
        // maxRecords=500 is a conservative deployed value known to work.
        // It must NOT fire the warning — this is the honest floor.
        let config = base_config(
            500,
            vec![
                tf(
                    "markets_virtual",
                    "base",
                    None,
                    vec![expand("markets", &["id", "name"])],
                ),
                tf(
                    "selections",
                    "markets_virtual",
                    Some("selections_clean"),
                    vec![],
                ),
            ],
        );
        let report = run_pipeline_lints(&config);
        assert!(
            report.is_empty(),
            "unexpected findings: {:?}",
            report.findings
        );
    }

    #[test]
    fn explode_at_exactly_safe_threshold_stays_silent() {
        // Boundary check: the SAFE threshold is inclusive-safe (records
        // at exactly this value don't overflow under safe traffic).
        let n = ArrowI32Overflow::SAFE_OVERFLOW_THRESHOLD_RECORDS;
        let config = base_config(
            n,
            vec![tf(
                "markets_virtual",
                "base",
                None,
                vec![expand("markets", &["id"])],
            )],
        );
        let report = run_pipeline_lints(&config);
        assert!(
            report.is_empty(),
            "unexpected findings at boundary: {:?}",
            report.findings
        );
    }

    #[test]
    fn explode_above_safe_threshold_warns_and_names_the_explode_node() {
        // A pathological value (3000) historically observed to trigger
        // the overflow. Should warn, and the warning should name the
        // sub_transform that does the JSON explode so operators can
        // trace it.
        let config = base_config(
            3_000,
            vec![
                tf(
                    "markets_virtual",
                    "base",
                    None,
                    vec![expand("markets", &["id", "name"])],
                ),
                tf(
                    "selections",
                    "markets_virtual",
                    Some("selections_clean"),
                    vec![],
                ),
            ],
        );
        let report = run_pipeline_lints(&config);
        assert_eq!(report.findings.len(), 1, "findings: {:?}", report.findings);
        let f = &report.findings[0];
        assert_eq!(f.id, LintId::ArrowI32FanoutRisk);
        assert_eq!(f.severity, PipelineSeverity::Warning);
        assert!(f.related_nodes.contains(&"markets_virtual".to_string()));
        assert!(!report.has_error(), "warning must not count as error");
    }

    #[test]
    fn same_warning_fires_for_higher_pre_cap_value() {
        // A second historically-deployed pathological value (6000).
        // Well above the threshold — must warn just like 3000 did.
        let config = base_config(
            6_000,
            vec![tf(
                "markets_virtual",
                "base",
                None,
                vec![expand("markets", &["id"])],
            )],
        );
        let report = run_pipeline_lints(&config);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].severity, PipelineSeverity::Warning);
    }

    #[test]
    fn message_carries_arithmetic_and_runtime_reference() {
        // The whole point of this lint is to explain the failure mode
        // *and* point at the durable fix. Losing either half in a future
        // refactor would silently degrade its usefulness — regress on
        // that here.
        let config = base_config(
            3_000,
            vec![tf(
                "markets_virtual",
                "base",
                None,
                vec![expand("markets", &["id"])],
            )],
        );
        let report = run_pipeline_lints(&config);
        let msg = &report.findings[0].message;
        assert!(
            msg.contains("i32 offset"),
            "missing runtime-failure-mode reference: {msg}"
        );
        assert!(
            msg.contains("upstream"),
            "missing upstream-contract framing: {msg}"
        );
        assert!(msg.contains("285 B"), "missing arithmetic constant: {msg}");
        assert!(
            msg.contains("jsonExpandColumns"),
            "missing failure-mode name: {msg}"
        );
        assert!(
            msg.contains("LargeStringArray"),
            "missing durable-fix reference: {msg}"
        );
    }

    #[test]
    fn missing_batch_config_produces_no_finding() {
        // When batch is entirely unset, we have no signal to fire on and
        // no default to assert against — silence is correct.
        let config = PipelineConfig {
            transform: PrimaryTransform {
                alias: "base".to_string(),
                sql_file: "base.sql".to_string(),
                on_error: None,
                missing_column_mode: None,
                schema_hint_columns: vec![],
            },
            dq: None,
            sink: vec![],
            sub_transforms: vec![tf(
                "markets_virtual",
                "base",
                None,
                vec![expand("markets", &["id"])],
            )],
            batch: None,
            dlq: None,
        };
        let report = run_pipeline_lints(&config);
        assert!(
            report.is_empty(),
            "unexpected findings: {:?}",
            report.findings
        );
    }
}
