-- Consumes this node's own json_expand_columns declaration on `accounts`.
SELECT
  id AS order_id,
  accounts_id AS account_id,
  accounts_tier AS tier,
  accounts_region AS region,
  _ssync_ingest_ts
FROM source
WHERE accounts IS NOT NULL
