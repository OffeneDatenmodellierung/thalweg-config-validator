-- Residual attributes not covered by the other leaves.
SELECT
  id,
  promo_code,
  event_ts_month,
  _ssync_source_headers,
  _ssync_producer
FROM source
