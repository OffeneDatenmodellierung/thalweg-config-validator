# Deployment Plan (Standalone Validator)

## Phase 0: Alignment and Contract Freeze

**Objective:** lock the config/rule contract before coding.

1. Capture `SubTransforms` TOML schema (fields, defaults, virtual semantics, order rules).
2. Capture SQL policy:
   - disallow joins
   - define allowed filters/operators/functions
   - define disallowed constructs
3. Capture lineage seed policy:
   - raw image defaults
   - meta columns
4. Publish a versioned contract file in repo (`contract/v1`).

**Exit criteria:** approved contract with examples and expected diagnostics.

## Phase 1: Bootstrap Crate and CI

**Objective:** establish a reproducible Rust 1.94 baseline.

1. Initialize crate/workspace.
2. Pin toolchain to Rust 1.94.
3. Add CI:
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
   - `cargo test`
4. Add fixture loading and snapshot/golden test harness.

**Exit criteria:** empty-pipeline green on main branch.

## Phase 2: Config + Rule Validation

**Objective:** validate TOML shape and transform rules.

1. Implement config parsing and structural validation.
2. Implement rule engine (no joins + allowlist constraints).
3. Emit structured diagnostics with transform/table path metadata.

**Exit criteria:** fixtures show expected pass/fail for rule-only scenarios.

## Phase 3: SQL Planning + Schema Inference

**Objective:** verify SQL and infer output schemas.

1. Parse SQL via DataFusion SQL parser.
2. Build logical plans using prior-transform/seed schemas.
3. Detect missing/unresolved columns.
4. Emit inferred output schemas and generated CREATE DDL.

**Exit criteria:** golden tests confirm schema ordering/types and DDL output.

## Phase 4: Column Lineage

**Objective:** full column traceability to lower-level sources.

1. Seed lineage graph from raw/default/meta columns.
2. Propagate lineage through projections/aliases/expressions.
3. Mark output columns untraceable when ancestry is unresolved.
4. Elevate table status to red on blocking lineage failures.

**Exit criteria:** lineage fixtures validate success and red-column failures.

## Phase 5: UI Projection Contract

**Objective:** provide app-ready tab state model.

1. Produce one tab per output table in transform order.
2. Add banner state calculation:
   - green all-ok
   - red any error
   - blue virtual marker
3. Add per-tab view toggles:
   - table output view model
   - DDL view model

**Exit criteria:** deterministic JSON contract suitable for front-end tab rendering.

## Phase 6: Release and Rollout

**Objective:** deploy safely with observability and rollback.

1. Tag release candidate (`v0.x`).
2. Run integration pack on representative configs.
3. Publish binaries/artifacts.
4. Roll out by environment:
   - dev
   - test
   - prod
5. Define rollback trigger and procedure:
   - validation false-positive spike
   - runtime failure
   - schema mismatch regression

**Exit criteria:** production enablement with rollback readiness.

## Runtime/Operational Controls

- Strict mode toggle (fail on warnings vs fail on errors only)
- Deterministic output checksum for CI diffing
- Structured logs for diagnostics and table status counts
- Contract version stamping in every output payload
