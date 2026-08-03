-- STAGE 1: light prep. BINARY -> VARCHAR casts + get_json_object extraction
-- to typed columns. Heavy string-function derivations are deferred to
-- prepared.sql (stage 2) so the optimizer never pushes them back onto a
-- BINARY source column.
SELECT
  CAST(get_json_object(_raw_payload, '$.orderId') AS STRING) AS id,
  CAST(get_json_object(_raw_payload, '$.customerId') AS STRING) AS customer_id,
  CAST(get_json_object(_raw_payload, '$.orderTotal') AS STRING) AS order_total_str,
  CAST(get_json_object(_raw_payload, '$.promoCode') AS STRING) AS promo_code,
  CAST(get_json_object(_raw_payload, '$.loyaltyPointsEarned') AS STRING) AS loyalty_points_earned_str,
  CAST(get_json_object(_raw_payload, '$.status') AS STRING) AS status,
  get_json_object(_raw_payload, '$.items') AS items,
  get_json_object(_raw_payload, '$.account') AS accounts,
  CAST(_ssync_source_topic AS STRING) AS source_topic_str,
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
  _raw_payload
FROM source
WHERE _ssync_source_topic IS NOT NULL OR _raw_payload IS NULL
