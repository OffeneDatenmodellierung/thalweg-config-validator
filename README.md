# thalweg-config-validator

A standalone Rust 1.94 binary crate for validating streaming-pipeline transform configurations. It parses a `config.yaml` (or `.toml`) containing a `transforms.base` block and a `subTransforms` list, then validates each transform's SQL file against the inferred upstream schema using DataFusion SQL planning. The tool enforces structural rules such as no-JOIN constraints, resolves `missingColumnMode` downgrade behaviour, and expands `jsonExpandColumns` synthetic columns into the output schema.

Column lineage is traced from a canonical raw/meta seed registry (all `_ssync_*` and `_raw_*` columns) through every transform in execution order. Each output column is resolved to a seed origin, a `schemaHintColumn` patch, or a prior transform's output — any column that cannot be traced is flagged as untraceable. Virtual transforms (those with no `cleanTable`/`destinationBackend` referenced as input by downstream transforms) are identified and labelled separately.

The validator emits a per-table schema summary with green/red/virtual status banners, a column-level lineage grid, generated CREATE DDL text, and a structured JSON report. See the docs below for the full contract, phased delivery plan, and worked examples.

On top of per-transform validation, the tool also runs a small set of **pipeline-level lints** — invariants that span the whole config (e.g. `batch.maxRecords` sizing vs. `jsonExpandColumns` fanout-overflow risk against Arrow's i32 offset ceiling). These are pure functions of the parsed config (no I/O, no SQL planning) and surface above the per-table blocks with stable kebab-case IDs like `arrow-i32-fanout-risk`, so operators can grep the framework issue tracker or docs by ID rather than by message text. See the *Pipeline-Level Lints* section of `docs/DELIVERABLES.md` for the current catalog and the severity policy.

## Contents

- [`docs/DELIVERABLES.md`](docs/DELIVERABLES.md) — module breakdown, full config contract, seed registry, lineage rules, UI model contract, and acceptance criteria
- [`docs/DEPLOYMENT_PLAN.md`](docs/DEPLOYMENT_PLAN.md) — phased delivery plan (scaffold → lineage → schema emitter → CLI → CI hardening → release)
- [`docs/EXAMPLES.md`](docs/EXAMPLES.md) — annotated config YAML, SQL transform snippets, JSON output, error/warning messages, DDL view, and CLI invocation

## Public-safe constraints

All examples and snippets in this repository are generic and contain no internal or company-identifying information.
