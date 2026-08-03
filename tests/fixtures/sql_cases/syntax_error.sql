-- EXPECT: hard error, category "syntax". Unterminated string literal - a
-- lexer-level failure, unlike a trailing comma which some sqlparser
-- versions tolerate leniently.
SELECT
  id,
  customer_id
FROM prepared
WHERE status = 'unterminated
