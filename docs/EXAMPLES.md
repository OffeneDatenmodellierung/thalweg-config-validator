# Thalweg Config Validator — Examples

## Example Config YAML

```yaml
transforms:
  base:
    alias: base
    sqlFile: /transforms/base_prep.sql
    onError: warn
    missingColumnMode: null_and_warn
    schemaHintColumns:
      - name: scheduling_start_time
        type: BIGINT
      - name: status_value
        type: STRING
      - name: properties_group_type
        type: STRING
      - name: participants
        type: STRING
      - name: markets
        type: STRING
  subTransforms:
    - name: prepared
      input: base
      sqlFile: /transforms/prepared.sql
    - name: entities
      input: prepared
      sqlFile: /transforms/entities.sql
      cleanTable: dev_catalog.silver.entities
      clusterBy: ["event_ts", "entity_id"]
      destinationBackend: zerobus
    - name: results
      input: prepared
      sqlFile: /transforms/results.sql
      cleanTable: dev_catalog.silver.results
      clusterBy: ["event_ts", "entity_id"]
      destinationBackend: zerobus
    - name: markets_virtual
      input: prepared
      sqlFile: /transforms/markets_explode.sql
      jsonExpandColumns:
        - name: markets
          fields: ["id", "name", "status", "selections"]
    - name: markets
      input: markets_virtual
      sqlFile: /transforms/markets.sql
      cleanTable: dev_catalog.silver.markets
      clusterBy: ["event_ts", "market_id", "entity_id"]
      destinationBackend: zerobus
    - name: selections
      input: markets_virtual
      sqlFile: /transforms/selections.sql
      cleanTable: dev_catalog.silver.selections
      clusterBy: ["selection_id", "market_id", "entity_id"]
      destinationBackend: zerobus
      jsonExpandColumns:
        - name: selections
          fields: ["id", "name", "status", "probability"]
```

## Example SQL Transform (no JOIN allowed)

```sql
-- prepared.sql
SELECT
    _ssync_record_id,
    _ssync_source_event_ts,
    _ssync_ingest_ts,
    CAST(_ssync_source_event_ts AS DATE)         AS event_ts_month,
    get_json_object(_raw_payload, '$.id')        AS entity_id,
    get_json_object(_raw_payload, '$.name')      AS entity_name,
    get_json_object(_raw_payload, '$.status')    AS entity_status,
    CAST(scheduling_start_time AS TIMESTAMP_NTZ) AS scheduled_start_time,
    participants,
    markets
FROM base
WHERE _raw_payload IS NOT NULL
```

## Example: JSON Expand Synthetic Columns

Given `jsonExpandColumns` on `markets_virtual`:
```yaml
jsonExpandColumns:
  - name: markets
    fields: ["id", "name", "status"]
```

Output schema of `markets_virtual` will include:
- `markets_id` (STRING)
- `markets_name` (STRING)
- `markets_status` (STRING)

These are valid lineage origins for downstream `markets` and `selections` transforms.

## Example: Validation Output JSON

```json
{
  "summary": [
    {
      "table": "prepared",
      "virtual": false,
      "status": "ok",
      "diagnostics": [],
      "columns": [
        { "name": "_ssync_record_id", "type": "STRING", "lineage": "ok", "origin": "seed:_ssync_record_id" },
        { "name": "entity_id", "type": "STRING", "lineage": "ok", "origin": "expr:get_json_object(_raw_payload)" }
      ]
    },
    {
      "table": "markets_virtual",
      "virtual": true,
      "status": "ok",
      "diagnostics": [],
      "columns": [
        { "name": "markets_id", "type": "STRING", "lineage": "ok", "origin": "json_expand:markets.id" },
        { "name": "markets_name", "type": "STRING", "lineage": "ok", "origin": "json_expand:markets.name" }
      ]
    },
    {
      "table": "markets",
      "virtual": false,
      "status": "error",
      "diagnostics": [
        { "code": "UNTRACEABLE_COLUMN", "column": "mystery_col", "message": "Column 'mystery_col' cannot be traced to any upstream source" }
      ],
      "columns": [
        { "name": "market_id", "type": "STRING", "lineage": "ok", "origin": "json_expand:markets.id" },
        { "name": "mystery_col", "type": "UNKNOWN", "lineage": "error", "origin": null }
      ]
    }
  ]
}
```

## Example: Validation Error — JOIN Detected

```
ERROR [markets.sql] line 4: JOIN is not permitted in SubTransform SQL.
  Found: LEFT JOIN other_table ON markets_virtual.market_id = other_table.id
  Rule: SubTransforms may only SELECT/FILTER/PROJECT from a single input table.
  Fix: Remove the JOIN; pre-compute joined columns in a prior transform.
```

## Example: Validation Warning — missingColumnMode

```
WARN [base] Column 'properties_group_type' not found in upstream schema.
  missingColumnMode=null_and_warn: synthesising NULL STRING column and continuing.
```

## Example: CREATE DDL View

```sql
CREATE TABLE markets (
    _ssync_record_id       STRING        NOT NULL,
    _ssync_source_event_ts TIMESTAMP_NTZ,
    event_ts_month         DATE,
    entity_id              STRING,
    market_id              STRING,
    market_name            STRING,
    market_status          STRING,
    mystery_col            UNKNOWN       -- LINEAGE ERROR: untraceable
);
```

## Example: CLI Invocation

```bash
thalweg-validate \
  --config ./config.yaml \
  --transforms-dir ./transforms \
  --output ./validation-report.json
```

## Example: rust-toolchain.toml

```toml
[toolchain]
channel = "1.94"
```
