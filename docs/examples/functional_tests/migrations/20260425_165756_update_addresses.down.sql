ALTER TABLE users DROP CONSTRAINT fk_users_address_id;

ALTER TABLE orders DROP CONSTRAINT fk_orders_customer_id;

DROP TABLE users;

DROP TABLE orders;

DROP TABLE addresses;
