\set ON_ERROR_STOP on

CREATE SCHEMA IF NOT EXISTS bench;
DROP TABLE IF EXISTS bench.rows;
CREATE TABLE bench.rows (
    row_no bigint NOT NULL,
    match_key text NOT NULL,
    payload text NOT NULL
);

INSERT INTO bench.rows (row_no, match_key, payload)
SELECT
    value,
    CASE
        WHEN value = 1 THEN 'early'
        WHEN value = ((:row_count)::bigint + 1) / 2 THEN 'middle'
        WHEN value = (:row_count)::bigint THEN 'late'
        ELSE 'row-' || value::text
    END,
    repeat(
        md5(value::text || ':pgdumpx:' || ((value * 1103515245 + 12345) % 2147483647)::text),
        8
    )
FROM generate_series(1, (:row_count)::bigint) AS generated(value);

ANALYZE bench.rows;
