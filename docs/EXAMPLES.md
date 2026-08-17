# Public-Safe Examples (No Company Identifiers)

> **Note:** All examples in this document use anonymized, generic e-commerce domain concepts (orders, customers, products, shipments). These are neutral, public-safe examples that preserve the technical semantics of the validator without revealing any internal business domain or company-identifying information.

## 1. Example Input Config (TOML)

```toml
[pipeline]
name = "demo_validator"

[[sub_transforms]]
id = "stg_orders"
output_table = "stg_orders"
virtual = false
sql = """
SELECT
  order_id,
  customer_id,
  order_total,
  source_ingest_ts AS ingest_ts,
  _meta_source_file
FROM raw_orders
WHERE order_total > 0
"""

[[sub_transforms]]
id = "v_orders_filtered"
output_table = "v_orders_filtered"
virtual = true
sql = """
SELECT
  order_id,
  customer_id,
  order_total,
  ingest_ts,
  _meta_source_file
FROM stg_orders
WHERE customer_id IS NOT NULL
"""

[[sub_transforms]]
id = "agg_customer_summary"
output_table = "agg_customer_summary"
virtual = false
sql = """
SELECT
  customer_id,
  COUNT(order_id) AS total_orders,
  SUM(order_total) AS lifetime_value,
  MAX(ingest_ts) AS last_order_ts
FROM stg_orders
WHERE customer_id IS NOT NULL
GROUP BY customer_id
"""
```

## 2. Example Seed Schema Inputs

```json
{
  "raw_orders": [
    {"name": "order_id", "type": "Utf8", "nullable": false},
    {"name": "customer_id", "type": "Utf8", "nullable": true},
    {"name": "order_total", "type": "Float64", "nullable": true},
    {"name": "source_ingest_ts", "type": "Timestamp(Nanosecond, None)", "nullable": true},
    {"name": "_meta_source_file", "type": "Utf8", "nullable": true}
  ]
}
```

## 3. Rule Policy Snippet

```yaml
sql_rules:
  joins_allowed: false
  allowed_filters:
    - "="
    - "!="
    - ">"
    - ">="
    - "<"
    - "<="
    - "IS NULL"
    - "IS NOT NULL"
  disallowed_clauses:
    - "JOIN"
    - "UNION"
```

## 4. Example schemaHintColumns (Handling Missing Columns)

```toml
[[sub_transforms]]
id = "stg_products"
output_table = "stg_products"
virtual = false
missing_column_mode = "null_and_warn"
schema_hint_columns = [
  { name = "discount_pct", type = "Float64", nullable = true }
]
sql = """
SELECT
  product_id,
  product_name,
  base_price,
  discount_pct,
  _meta_source_file
FROM raw_products
WHERE base_price > 0
"""
```

**Behavior:** If `discount_pct` is missing from `raw_products`, the validator:
- Injects the column with `NULL` values per the hint
- Emits a warning diagnostic
- Allows the transform to succeed (instead of hard-failing)

## 5. Example No-Join Policy Violation

```toml
[[sub_transforms]]
id = "invalid_join"
output_table = "invalid_join"
virtual = false
sql = """
SELECT
  o.order_id,
  c.customer_name
FROM raw_orders o
JOIN raw_customers c ON o.customer_id = c.customer_id
"""
```

**Expected Diagnostic:**
```json
{
  "transform": "invalid_join",
  "severity": "error",
  "category": "rule",
  "message": "JOIN clauses are not allowed per pipeline policy."
}
```

## 6. Example Validation Output (Tab Model)

```json
{
  "table_tabs": [
    {
      "table": "stg_orders",
      "virtual": false,
      "banner": "green",
      "view_modes": ["table_output", "create_ddl"],
      "columns": [
        {"name": "order_id", "type": "Utf8", "lineage_status": "ok"},
        {"name": "customer_id", "type": "Utf8", "lineage_status": "ok"},
        {"name": "order_total", "type": "Float64", "lineage_status": "ok"},
        {"name": "ingest_ts", "type": "Timestamp(Nanosecond, None)", "lineage_status": "ok"},
        {"name": "_meta_source_file", "type": "Utf8", "lineage_status": "ok"}
      ]
    },
    {
      "table": "v_orders_filtered",
      "virtual": true,
      "banner": "green",
      "virtual_banner": "blue",
      "view_modes": ["table_output", "create_ddl"],
      "columns": [
        {"name": "order_id", "type": "Utf8", "lineage_status": "ok"},
        {"name": "customer_id", "type": "Utf8", "lineage_status": "ok"},
        {"name": "order_total", "type": "Float64", "lineage_status": "ok"},
        {"name": "ingest_ts", "type": "Timestamp(Nanosecond, None)", "lineage_status": "ok"},
        {"name": "_meta_source_file", "type": "Utf8", "lineage_status": "ok"}
      ]
    },
    {
      "table": "agg_customer_summary",
      "virtual": false,
      "banner": "green",
      "view_modes": ["table_output", "create_ddl"],
      "columns": [
        {"name": "customer_id", "type": "Utf8", "lineage_status": "ok"},
        {"name": "total_orders", "type": "Int64", "lineage_status": "ok"},
        {"name": "lifetime_value", "type": "Float64", "lineage_status": "ok"},
        {"name": "last_order_ts", "type": "Timestamp(Nanosecond, None)", "lineage_status": "ok"}
      ]
    }
  ]
}
```

## 7. Example CREATE DDL View Output

```sql
-- Generated DDL for stg_orders
CREATE TABLE stg_orders (
  order_id VARCHAR NOT NULL,
  customer_id VARCHAR,
  order_total DOUBLE,
  ingest_ts TIMESTAMP,
  _meta_source_file VARCHAR
);

-- Generated DDL for agg_customer_summary
CREATE TABLE agg_customer_summary (
  customer_id VARCHAR NOT NULL,
  total_orders BIGINT NOT NULL,
  lifetime_value DOUBLE,
  last_order_ts TIMESTAMP
);
```

## 8. Example Red Banner (Lineage Failure)

```json
{
  "table": "stg_enriched_orders",
  "virtual": false,
  "banner": "red",
  "view_modes": ["table_output", "create_ddl"],
  "columns": [
    {"name": "order_id", "type": "Utf8", "lineage_status": "ok"},
    {"name": "customer_id", "type": "Utf8", "lineage_status": "ok"},
    {"name": "unknown_flag", "type": "Boolean", "lineage_status": "error"}
  ],
  "diagnostics": [
    {
      "column": "unknown_flag",
      "severity": "error",
      "category": "lineage",
      "message": "Column cannot be traced to lower-level sources or seed columns."
    }
  ]
}
```

## 9. Example Untraceable Column Diagnostic

```json
{
  "table": "stg_orders",
  "column": "unknown_flag",
  "severity": "error",
  "category": "lineage",
  "message": "Column cannot be traced to lower-level sources or seed columns."
}
```

## 10. Complete Transform Chain Example (Lineage Demonstration)

This example demonstrates full column lineage tracing through a multi-stage transform pipeline.

### Seed Schema (raw_shipments)
```json
{
  "raw_shipments": [
    {"name": "shipment_id", "type": "Utf8", "nullable": false},
    {"name": "order_id", "type": "Utf8", "nullable": false},
    {"name": "warehouse_code", "type": "Utf8", "nullable": true},
    {"name": "ship_date", "type": "Date32", "nullable": true},
    {"name": "carrier", "type": "Utf8", "nullable": true},
    {"name": "_raw_ingest_ts", "type": "Timestamp(Nanosecond, None)", "nullable": true},
    {"name": "_meta_source_file", "type": "Utf8", "nullable": true}
  ]
}
```

### Transform Pipeline
```toml
[[sub_transforms]]
id = "bronze_shipments"
output_table = "bronze_shipments"
virtual = false
sql = """
SELECT
  shipment_id,
  order_id,
  warehouse_code,
  ship_date,
  carrier,
  _raw_ingest_ts AS ingest_ts,
  _meta_source_file
FROM raw_shipments
WHERE shipment_id IS NOT NULL
"""

[[sub_transforms]]
id = "silver_shipments"
output_table = "silver_shipments"
virtual = false
sql = """
SELECT
  shipment_id,
  order_id,
  UPPER(warehouse_code) AS warehouse_code,
  ship_date,
  LOWER(carrier) AS carrier_normalized,
  ingest_ts,
  _meta_source_file
FROM bronze_shipments
WHERE ship_date IS NOT NULL
"""

[[sub_transforms]]
id = "v_shipments_by_warehouse"
output_table = "v_shipments_by_warehouse"
virtual = true
sql = """
SELECT
  warehouse_code,
  COUNT(shipment_id) AS shipment_count,
  MIN(ship_date) AS first_shipment_date,
  MAX(ship_date) AS last_shipment_date
FROM silver_shipments
GROUP BY warehouse_code
"""
```

### Lineage Trace Result

All columns in the final outputs trace back to:
- **Raw columns**: `shipment_id`, `order_id`, `warehouse_code`, `ship_date`, `carrier`
- **Default seed columns**: `_raw_ingest_ts` (renamed to `ingest_ts`)
- **Meta seed columns**: `_meta_source_file`

Derived columns (`carrier_normalized`, `shipment_count`, `first_shipment_date`, `last_shipment_date`) are traced to their source expressions and marked `lineage_status: "ok"` because all inputs are traceable.

## 11. Summary Dataset Output Example

```json
{
  "validator_version": "1.0.0",
  "contract_version": "v1",
  "pipeline_name": "demo_validator",
  "final_tables": [
    {
      "table": "stg_orders",
      "virtual": false,
      "status": "ok",
      "columns": [
        {"name": "order_id", "type": "Utf8"},
        {"name": "customer_id", "type": "Utf8"},
        {"name": "order_total", "type": "Float64"},
        {"name": "ingest_ts", "type": "Timestamp(Nanosecond, None)"},
        {"name": "_meta_source_file", "type": "Utf8"}
      ]
    },
    {
      "table": "v_orders_filtered",
      "virtual": true,
      "status": "ok",
      "columns": [
        {"name": "order_id", "type": "Utf8"},
        {"name": "customer_id", "type": "Utf8"},
        {"name": "order_total", "type": "Float64"},
        {"name": "ingest_ts", "type": "Timestamp(Nanosecond, None)"},
        {"name": "_meta_source_file", "type": "Utf8"}
      ]
    },
    {
      "table": "agg_customer_summary",
      "virtual": false,
      "status": "ok",
      "columns": [
        {"name": "customer_id", "type": "Utf8"},
        {"name": "total_orders", "type": "Int64"},
        {"name": "lifetime_value", "type": "Float64"},
        {"name": "last_order_ts", "type": "Timestamp(Nanosecond, None)"}
      ]
    }
  ],
  "diagnostics_summary": {
    "total_errors": 0,
    "total_warnings": 0,
    "by_category": {
      "syntax": 0,
      "rule": 0,
      "schema": 0,
      "lineage": 0
    }
  }
}
```

## 11. Example Pipeline-Level Lint (Arrow i32 Fanout-Risk)

Pipeline-level lints render above per-table blocks in the human-readable
report and under a top-level `pipeline_lints` array in JSON. See
[DELIVERABLES → Pipeline-Level Lints](./DELIVERABLES.md#pipeline-level-lints)
for the full catalog.

**Config input (excerpt):**

```yaml
subTransforms:
  - name: markets_virtual
    input: prepared
    sqlFile: /transforms/markets_explode.sql
    jsonExpandColumns:
      - name: markets
        fields: ["id", "name", "status"]
  - name: markets
    input: markets_virtual
    sqlFile: /transforms/markets.sql
    cleanTable: markets_clean

batch:
  maxWaitMs: 100
  maxRecords: 3000        # ← above the 1177 fanout-overflow threshold
  maxBytes: 20971520
```

**Human-readable output (excerpt):**

```
Pipeline validation: GREEN

Pipeline-level lints:
  - [warning/arrow-i32-fanout-risk] batch.maxRecords=3000 is above the
    1177-record threshold at which a batch of upstream-compliant records
    can overflow Arrow's i32 offset ceiling post-jsonExpandColumns.
    Arithmetic: post-explode column bytes = input_rows ×
    selections_per_record × 285 B. At the upstream-safe ceiling of 6400
    selections/record, overflow starts at N > 2^31 / (6400 × 285) = 1177.
    Underlying runtime issue: the stream-sync `explode_json_column`
    implementation emits Utf8 arrays with i32 offsets; the durable fix
    is LargeStringArray/i64 offsets or post-explode chunking.
    Recommended cap until that ships: maxRecords ≤ 1177.
    Explode nodes: [markets_virtual].
    related nodes: markets_virtual

[--] base (primary transform)
    ...
```

**JSON output (excerpt):**

```json
{
  "overall_status": "green",
  "pipeline_lints": [
    {
      "id": "arrow-i32-fanout-risk",
      "severity": "warning",
      "message": "batch.maxRecords=3000 is above the 1177-record threshold...",
      "related_nodes": ["markets_virtual"]
    }
  ],
  "tables": [ /* ... per-table blocks ... */ ]
}
```

**Note on severity:** the lint is a `warning`, not an `error`. The
config is legitimate at deploy time; the finding is a preflight
reminder that a known bug in the downstream runtime will bite this
config's traffic profile under bursty replay. `overall_status` stays
`green` and the CLI exits 0 — so CI/CD gates keep passing while the
finding is surfaced to reviewers. When the underlying runtime fix
ships and the lint is no longer relevant, remove it (or gate it on a
runtime version constraint) rather than escalating to `error`.
