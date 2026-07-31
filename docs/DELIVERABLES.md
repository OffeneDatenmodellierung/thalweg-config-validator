# Deliverables: Standalone Config + DataFusion SQL Validator

## 1. Product Deliverable

Build a standalone Rust **1.94** application that validates transform config and SQL, computes output schemas, and emits UI-ready tab state per output table.

## 2. Functional Deliverables

1. **Config contract ingestion**
   - Parse TOML config with explicit schema for `SubTransforms`
   - Validate required fields, defaults, ordering, and virtual output flags

2. **Rule validation layer**
   - Enforce processing constraints (for example: no joins)
   - Validate allowed filter/operator/function set
   - Surface clear diagnostics with table/transform context

3. **SQL validation (DataFusion)**
   - Parse SQL for syntax errors
   - Build logical plans against known upstream schemas
   - Detect unresolved/missing columns

4. **Schema inference**
   - Infer final output schema per transform output
   - Preserve column order and types
   - Emit deterministic CREATE DDL text for each output

5. **Lineage engine**
   - Seed lineage from raw/default/meta columns
   - Propagate lineage through each transform step
   - Mark untraceable output columns as red/error

6. **UI model output**
   - One tab per output table, in transform order
   - Banner logic:
     - Green: all table checks pass
     - Red: any blocking error exists
     - Blue virtual banner: table is virtual (in addition to status)
   - Dual view mode:
     - Table Output view
     - CREATE DDL view

7. **Summary dataset output**
   - Consolidated final table schemas
   - Per-table status and diagnostics
   - Per-column lineage trace status

## 3. Technical Deliverables

1. Rust crate layout (standalone)
   - `config_contract`
   - `rule_engine`
   - `sql_validator`
   - `schema_inference`
   - `lineage_engine`
   - `ui_projection`
   - `cli` (entrypoint and outputs)

2. Stable output contracts
   - JSON output schema for machine consumption
   - Optional human-readable report rendering

3. Test assets
   - Fixture configs and SQL samples (public-safe)
   - Golden outputs for schema and diagnostics

## 4. Non-Functional Deliverables

- Deterministic output for identical input
- No runtime dependency on internal stream-sync crates
- Clear error taxonomy (`syntax`, `rule`, `schema`, `lineage`)
- Rustfmt/clippy clean
- CI pipeline for build/test/lint

## 5. Acceptance Criteria

1. Inputs are TOML config + seed schemas only.
2. SQL syntax failures are reported per transform.
3. Missing column errors are reported with source context.
4. Rule violations (including no-join policy) are reported.
5. All output tables (including virtual) are represented as tabs.
6. Banner states are correctly computed (green/red + blue virtual).
7. Each tab supports both table and DDL view models.
8. Untraceable columns are marked red at column level.
9. A final summary set of expected output schemas is emitted.
