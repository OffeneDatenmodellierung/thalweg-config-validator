# Thalweg Config Validator — Deployment Plan

## Phase 0 — Contract Freeze (Day 1)

- Extract config YAML structure from a live pipeline deployment.
- Snapshot raw/meta seed column registry.
- Export representative SQL transform files (sanitised, no production data).
- Freeze these as test fixtures under `tests/fixtures/`.

## Phase 1 — Scaffold & Core Crates (Days 2–5)

- `cargo new --bin thalweg-config-validator` (Rust 1.94 toolchain pinned in `rust-toolchain.toml`).
- Add DataFusion SQL crates to `Cargo.toml`.
- Implement `config_contract`, `seed_registry`, `sql_validator` modules.
- Passing unit tests for parse and syntax validation.

## Phase 2 — Lineage Engine (Days 6–9)

- Implement `lineage_engine` with DAG traversal.
- Wire `jsonExpandColumns` synthetic column expansion.
- Implement `missingColumnMode: null_and_warn` downgrade logic.
- Passing unit tests for lineage success, untraceable-column red-flagging.

## Phase 3 — Schema Emitter & UI Model (Days 10–12)

- Implement `schema_emitter` (final schema + DDL generation).
- Implement `ui_model` (tab list, banner state, toggle view data).
- Integration test: full config fixture → expected tab/banner JSON output.

## Phase 4 — Reporter & CLI (Days 13–14)

- Implement `reporter` (stdout JSON + optional file output).
- CLI: `thalweg-validate --config <path> --transforms-dir <path> [--output <path>]`.
- End-to-end integration test with full fixture set.

## Phase 5 — Hardening & CI Gate (Days 15–17)

- `cargo fmt`, `cargo clippy --deny warnings`, `cargo audit`.
- Add GitHub Actions workflow: build + test on Rust 1.94 stable.
- Verify deterministic output for CI diff comparison.

## Phase 6 — Documentation & Release (Day 18)

- Final README update with usage examples.
- Tag `v0.1.0`.
- Confirm no company-identifying data in repo.

## Rollback

This is a standalone validator binary — no coupling to the downstream runtime it validates configs for. Rollback = deleting or not running the binary. No migrations, no schema changes, no service dependencies.
