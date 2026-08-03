-- EXPECT: hard error, category "rule". JOINs are never permitted, and this
-- must NOT be downgradable by on_error = "warn" (that setting only downgrades
-- schema/lineage findings, not structural rule violations).
SELECT
  p.id,
  a.tier
FROM prepared p
JOIN accounts a ON p.id = a.order_id
