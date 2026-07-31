# Public-Safe Examples (No Company Identifiers)

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

## 4. Example Validation Output (Tab Model)

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
    }
  ]
}
```

## 5. Example Untraceable Column Diagnostic

```json
{
  "table": "stg_orders",
  "column": "unknown_flag",
  "severity": "error",
  "category": "lineage",
  "message": "Column cannot be traced to lower-level sources or seed columns."
}
```
