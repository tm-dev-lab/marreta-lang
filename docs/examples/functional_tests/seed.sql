-- Functional test schema.
-- A single `items` table is enough to exercise every DB feature of MarretaLang.
-- Seeded with a small fixed dataset so tests can rely on known initial state.

CREATE TABLE IF NOT EXISTS items (
    id     SERIAL  PRIMARY KEY,
    name   TEXT    NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true
);

-- Four rows: three active, one inactive.
-- Tests that filter by active:true will always find at least 3 rows on a fresh DB.
INSERT INTO items (name, active) VALUES
    ('alpha',   true),
    ('beta',    true),
    ('gamma',   false),
    ('delta',   true);
