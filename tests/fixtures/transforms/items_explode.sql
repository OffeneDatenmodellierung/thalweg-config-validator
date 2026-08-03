-- Virtual node (no clean_table). Explodes the items JSON array; feeds
-- both the items leaf and (in a real pipeline) any per-item aggregations.
SELECT
  id AS order_id,
  items,
  _ssync_ingest_ts
FROM source
WHERE items IS NOT NULL
