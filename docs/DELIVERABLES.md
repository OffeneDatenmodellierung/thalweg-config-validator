# Thalweg Config Validator — Deliverables

## Overview

A standalone Rust 1.94 binary crate that:
1. Parses a stream-sync `config.yaml` (or `.toml`) containing a `transforms.base` block and a `subTransforms` list.
2. Validates each transform's SQL file against the inferred upstream schema using DataFusion SQL planning.
3. Traces column lineage from the raw/meta seed registry through every transform in execution order.
4. Emits a per-table schema summary with validation status and a UI model for tab/banner rendering.

---

## Module Breakdown

| Module | Responsibility |
|---|---|
| `config_contract` | Deserialise the full config YAML/TOML; model `base`, `subTransforms`, `schemaHintColumns`, `jsonExpandColumns`, `cleanTable`, `clusterBy`, `destinationBackend` |
| `seed_registry` | Owns the canonical raw/meta column table (all `_ssync_*` and `_raw_*` columns) with types and descriptions |
| `sql_validator` | DataFusion parse + logical plan check per SQL file; reports syntax errors, missing-column refs, and disallowed constructs (joins, unsupported functions) |
| `lineage_engine` | Propagates column provenance through the transform DAG; marks columns untraceable when origin cannot be resolved |
| `pipeline_lints` | Config-wide invariants that cross the subTransforms × batch boundary (e.g. batch-sizing vs. jsonExpandColumns fanout-overflow risk). Pure functions of the parsed config — no I/O, no SQL planning. |
| `schema_emitter` | Builds the final inferred schema per output table; generates CREATE DDL text |
| `ui_model` | Produces the tab/banner/toggle view model consumed by the rendering layer |
| `reporter` | Writes the summary JSON/table output to stdout or file, including pipeline-level lints above per-table findings |

---

## Config Contract

### `transforms.base`

```yaml
transforms:
  base:
    alias: base
    sqlFile: /transforms/base_prep.sql
    onError: warn
    missingColumnMode: null_and_warn
    schemaHintColumns:
      - name: sparse_field_a
        type: BIGINT
      - name: sparse_field_b
        type: STRING
```

**Validation rules for `base`:**
- `missingColumnMode: null_and_warn` downgrades missing columns to WARNING + NULL synth; does NOT hard-fail.
- `schemaHintColumns` entries are optional nullable patch columns; may safely be absent from upstream schema.
- `onError: warn` downgrades validation failures to warnings in the output report.

### `subTransforms` list

```yaml
subTransforms:
  - name: prepared
    input: base
    sqlFile: /transforms/prepared.sql
    cleanTable: dev_catalog.silver.prepared_clean
    clusterBy: ["event_ts", "entity_id"]
    destinationBackend: zerobus
    jsonExpandColumns:
      - name: markets
        fields: ["id", "name", "status"]
```

**Validation rules for `subTransforms`:**
- Execution order is list order; a transform can only reference a `name` or `alias` already processed.
- `input` must resolve to an already-validated output table or the `base` alias.
- **No SQL JOINs are permitted.** Any JOIN clause is a hard validation error.
- Allowed filter operators: `=`, `!=`, `<`, `>`, `<=`, `>=`, `IN`, `IS NULL`, `IS NOT NULL`, `LIKE`, `BETWEEN`, `AND`, `OR`.
- `jsonExpandColumns` entries synthesise additional columns (`<col>_<field>` pattern) into the output schema.
- A **virtual** table is any `subTransform` with no `cleanTable`/`destinationBackend` that is referenced as `input` by a downstream transform.
- Non-virtual tables with `cleanTable` and `destinationBackend` are physical sink tables.

---

## Raw/Meta Seed Column Registry

All transforms inherit the following columns as valid lineage origins.

| Column | Type | Description |
|---|---|---|
| `_raw_payload` | STRING | Raw message payload |
| `_raw_payload_bin` | BINARY | Binary raw payload |
| `_ssync_source_topic` | STRING | Source topic name |
| `_ssync_source_partition` | INT | Source partition index |
| `_ssync_source_offset` | STRING | Source message position |
| `_ssync_message_key` | STRING | Message key; NULL when unset |
| `_ssync_producer` | STRING | Producer name; NULL for Kafka |
| `_ssync_sequence_id` | BIGINT | Producer-assigned sequence id |
| `_ssync_source_headers` | STRING | Headers/properties as JSON string |
| `_ssync_record_id` | STRING | Deterministic per-record identity |
| `_ssync_source_event_ts` | TIMESTAMP_NTZ | Broker/producer event time |
| `_ssync_ingest_ts` | TIMESTAMP_NTZ | Wall-clock ingest time |
| `_ssync_emit_ts` | TIMESTAMP_NTZ | Wall-clock write time at sink; NULL on raw lane |

---

## Column Lineage Rules

1. Every output column must resolve to one of:
   a. A raw/meta seed column (table above).
   b. A `schemaHintColumn` from `base`.
   c. An output column from a prior transform in execution order.
   d. A DataFusion SQL expression entirely composed of traceable inputs.
2. If a column cannot be traced → mark column **RED** in UI; table banner turns **RED**.
3. Columns from `jsonExpandColumns` synthetic expansion are traceable iff the source JSON column is itself traceable.

---

## Pipeline-Level Lints

Invariants that live above any single transform's SQL and cross the
`subTransforms` × `batch` boundary are surfaced as pipeline-level
findings, rendered above the per-table blocks in the human-readable
report and under a top-level `pipeline_lints` array in the JSON report.

Each lint carries a stable kebab-case `id` so operators can grep the
issue tracker / docs by the ID rather than by message text.

### `arrow-i32-fanout-risk` (severity: `warning`)

**When it fires:** the config declares at least one `jsonExpandColumns`
sub-transform *and* `batch.maxRecords > 1177`.

**Why 1177:** the downstream stream-sync runtime's `explode_json_column`
implementation builds output Utf8 `StringArray`s with i32 offsets. Under
aggregation, the post-explode column bytes are approximately:

```
input_rows × selections_per_record × 285 B
```

Under a documented upstream publisher contract that caps nested-array
sizes at ≤6 400 elements per record (the "safe" band) and ≈285 B per
element uncompressed, overflow of Arrow's i32 offset (`i32::MAX`
≈ 2 GB) starts at:

```
N > 2^31 / (6 400 × 285) = 1 177 records
```

Any batch larger than this can OOM the runtime pod without any upstream
contract violation — no benign traffic mix saves it.

**Underlying runtime issue:** the durable fix in the downstream runtime
is `LargeStringArray` with i64 offsets, or post-explode chunking on
`RecordBatch` byte size. Until that ships, this lint is the preflight
defence against re-introducing the class of production incidents that
motivated it.

**Provenance in code:**
[`pipeline_lints::ArrowI32Overflow`](../src/pipeline_lints.rs) carries
the four inputs (upstream elements-per-record ceiling, per-element byte
size, Arrow i32 offset ceiling, derived threshold) as associated
constants with unit-test coverage guarding drift between the derivation
comment and the constant value. Adjust the two upstream constants (and
re-run the tests) if your pipeline's publishers document a different
contract — the derivation itself is invariant.

### Severity policy

- `warning` — the finding does **not** change `overall_status` from
  `green` to `red` and does **not** cause the CLI to exit non-zero.
  Suitable for known runtime workarounds where the config is legitimate
  at deploy time but should be revisited when the underlying fix lands.
- `error` — the finding **does** turn `overall_status` red and **does**
  cause a non-zero exit. Reserved for pipeline-wide invariants where
  no downstream mitigation applies (e.g. duplicate `cleanTable` across
  sub_transforms — not yet implemented; documented here as the
  reference case for the severity split).

Adding a new lint: implement the check as a private function inside
`src/pipeline_lints.rs` that pushes to `&mut Vec<PipelineFinding>`,
add a new `LintId` variant, invoke it from `run_pipeline_lints`, and
write a unit test per firing / non-firing case. Do **not** invent tier
boundaries that aren't grounded in the arithmetic — monotonic risk
curves get one threshold, not several.

---

## UI Model Contract

### Tab / Banner Rules

| Condition | Banner |
|---|---|
| All columns traceable, no SQL errors | Green |
| Any SQL error or untraceable column | Red |
| Table is virtual (no cleanTable/destinationBackend) | Blue "Virtual" banner shown in addition to green/red |

### Per-Tab Toggle Views

1. **Table Output View** — column grid: `column_name | data_type | nullable | lineage_status | origin_path`
2. **CREATE DDL View** — generated `CREATE TABLE <name> (...)` from inferred schema.

---

## Acceptance Criteria

- [ ] Parses full config YAML (base + subTransforms) without stream-sync runtime dependency.
- [ ] Validates SQL files in transform execution order.
- [ ] Enforces no-JOIN rule; syntax/missing-col errors surfaced with context.
- [ ] `missingColumnMode: null_and_warn` downgrades to warning + NULL synth; does not hard-fail.
- [ ] All raw/meta seed columns recognised as valid lineage origins.
- [ ] `schemaHintColumns` accepted as nullable patch origins.
- [ ] `jsonExpandColumns` synthetic columns propagated into downstream lineage.
- [ ] Virtual tables identified correctly; blue banner rendered.
- [ ] Green/red status banners reflect per-table validation health.
- [ ] Table Output and CREATE DDL toggle views generated per tab.
- [ ] Summary JSON report emitted for all output tables.
- [ ] Unit + integration tests for: valid SQL, syntax failure, missing col, no-join violation, virtual table, lineage success/failure, DDL generation, multi-transform ordering.
- [ ] Builds clean on Rust 1.94.
