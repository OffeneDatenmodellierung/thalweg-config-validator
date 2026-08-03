SELECT
  id,
  customer_id,
  order_total,
  promo_code,
  source_feed,
  event_ts_month,
  _ssync_record_id,
  _ssync_ingest_ts
FROM source
WHERE order_total > 0
