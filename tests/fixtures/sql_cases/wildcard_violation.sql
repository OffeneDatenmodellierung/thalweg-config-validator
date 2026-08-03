-- EXPECT: hard error, category "rule". SELECT * is never permitted - all
-- columns must be explicitly named. Must be caught BEFORE DataFusion
-- planning, since wildcards are expanded into named columns during
-- planning and are invisible to any post-plan check.
SELECT *
FROM prepared
