-- EXPECT: SQL is syntactically and semantically valid (DataFusion plans it
-- fine) - this is NOT a sql_validator error. But `unknown_flag` is a bare
-- literal: it resolves to none of the three permitted origins (seed column,
-- schemaHintColumn, or prior transform output), so lineage_engine must mark
-- it RED and turn the table banner red, per lineage rule 1.
SELECT
  id,
  customer_id,
  'v2' AS unknown_flag
FROM prepared
