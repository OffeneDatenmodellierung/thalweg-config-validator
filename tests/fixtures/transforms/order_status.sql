-- Volatile live state: latest snapshot per order, written on every update.
SELECT
  id,
  status,
  loyalty_points_earned,
  _ssync_ingest_ts AS updated_at
FROM source
WHERE status IS NOT NULL
