# thalweg-config-validator

Public-safe planning repository for a standalone Rust 1.94 validator that:

- Reads stream-style config `SubTransforms` from TOML
- Validates DataFusion SQL for syntax and semantic compatibility
- Detects missing columns and rule violations
- Infers final output schemas in transform order
- Tracks per-column lineage (including default and meta seed columns)
- Produces UI-ready table tab models with status banners and dual views

This repository currently contains implementation deliverables, deployment planning, and sanitized examples suitable for a public codebase.

## Contents

- `docs/DELIVERABLES.md` — delivery scope, module breakdown, acceptance criteria
- `docs/DEPLOYMENT_PLAN.md` — phased deployment plan with controls and rollout gates
- `docs/EXAMPLES.md` — non-company-identifying config and SQL examples

## Public-safe constraints

All examples and snippets in this repository are generic and contain no internal/company identifiers.
