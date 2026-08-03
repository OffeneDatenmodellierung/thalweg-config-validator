-- EXPECT: valid. All columns resolve against `prepared` upstream schema.
SELECT
  id,
  customer_id,
  order_total,
  status
FROM prepared
WHERE order_total > 0
