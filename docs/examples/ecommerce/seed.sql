-- Ecommerce example seed — runs once on first container start.

CREATE TABLE IF NOT EXISTS products (
    id       SERIAL PRIMARY KEY,
    name     TEXT    NOT NULL,
    price    NUMERIC(10, 2) NOT NULL DEFAULT 0,
    category TEXT    NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS orders (
    id              SERIAL PRIMARY KEY,
    billing_city    TEXT    NOT NULL,
    billing_street  TEXT    NOT NULL DEFAULT '',
    billing_zipcode TEXT    NOT NULL DEFAULT '',
    coupon          TEXT    NOT NULL DEFAULT 'NONE',
    discount_rate   NUMERIC(5, 4) NOT NULL DEFAULT 0,
    item_count      INTEGER NOT NULL DEFAULT 0,
    status          TEXT    NOT NULL DEFAULT 'pending',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Sample products
INSERT INTO products (name, price, category) VALUES
    ('Widget',  9.99,  'tools'),
    ('Gadget',  24.99, 'electronics'),
    ('Thingamajig', 4.50, 'accessories');
