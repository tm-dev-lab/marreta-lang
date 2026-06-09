ALTER TABLE users DROP CONSTRAINT fk_users_address_id;

ALTER TABLE users DROP COLUMN address_id;

ALTER TABLE users DROP COLUMN active;

DROP TABLE addresses;
