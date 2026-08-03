-- EXPECT: behavior depends on missing_column_mode (see missing_column_modes.json):
--   - mode absent/"fail"        -> hard error, category "schema"
--   - mode "null_and_warn"      -> WARNING + NULL synth for `loyalty_tier`; does not fail
-- `loyalty_tier` does not exist in the `prepared` upstream schema and is not
-- declared as a schema_hint_column here.
SELECT
  id,
  customer_id,
  loyalty_tier
FROM prepared
