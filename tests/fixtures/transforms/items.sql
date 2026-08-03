-- Consumes the synthetic <col>_<field> columns expanded from items_virtual's
-- json_expand_columns declaration on `items`.
SELECT
  order_id,
  items_id AS item_id,
  items_sku AS sku,
  items_quantity AS quantity,
  items_unit_price AS unit_price,
  _ssync_ingest_ts
FROM source
WHERE items_id IS NOT NULL
