-- STAGE 2 virtual node (no clean_table). Derives source_feed and typed
-- numeric/temporal columns on the VARCHAR columns materialised by base.
-- All columns explicitly named - no wildcards permitted.
SELECT
  id,
  customer_id,
  order_total_str,
  promo_code,
  loyalty_points_earned_str,
  status,
  items,
  accounts,
  source_topic_str,
  _ssync_source_partition,
  _ssync_source_offset,
  _ssync_message_key,
  _ssync_producer,
  _ssync_sequence_id,
  _ssync_source_headers,
  _ssync_record_id,
  _ssync_event_ts,
  _ssync_ingest_ts,
  _ssync_emit_ts,
  _raw_payload,
  UPPER(TRIM(SPLIT_PART(source_topic_str, '.', -1))) AS source_feed,
  CAST(order_total_str AS DOUBLE) AS order_total,
  CAST(loyalty_points_earned_str AS BIGINT) AS loyalty_points_earned,
  DATE_TRUNC('month', _ssync_event_ts) AS event_ts_month
FROM source
