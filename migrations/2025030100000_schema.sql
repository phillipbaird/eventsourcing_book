CREATE TABLE inventories (
    inventory INT NOT NULL,
    product_id UUID NOT NULL,
    last_event_id BIGINT NOT NULL,
    PRIMARY KEY (product_id)
);


CREATE TABLE kafka_topic (
    topic TEXT NOT NULL,
    last_offset BIGINT NOT NULL,
    PRIMARY KEY (topic)
);


CREATE TABLE carts_with_products (
    cart_id UUID NOT NULL,
    product_id UUID NOT NULL,
    item_id UUID NOT NULL,
    last_event_id BIGINT NOT NULL,
    PRIMARY KEY (cart_id, item_id, product_id)
);

CREATE INDEX index_carts_with_products_product_id
ON carts_with_products (product_id);


CREATE TABLE queue (
    task_id UUID PRIMARY KEY,
    task_type TEXT NOT NULL,
    triggering_event BIGINT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    scheduled_for TIMESTAMP NOT NULL,
    next_attempt_at TIMESTAMP NOT NULL,
    timeout_at TIMESTAMP NOT NULL,
    max_attempts INT NOT NULL,
    failed_attempts INT NOT NULL,
    status INT NOT NULL,
    domain_args JSONB NOT NULL,
    UNIQUE (task_type, triggering_event)
);
CREATE INDEX index_queue_scheduled_for ON queue (scheduled_for);
CREATE INDEX index_queue_status ON queue (status);

CREATE TABLE cart (
    cart_id UUID PRIMARY KEY
);

CREATE TABLE cart_items (
    cart_id UUID NOT NULL,
    description TEXT NOT NULL,
    image TEXT NOT NULL,
    price NUMERIC NOT NULL,
    item_id UUID NOT NULL,
    product_id UUID NOT NULL,
    fingerprint TEXT NOT NULL,
    last_event_id BIGINT NOT NULL,
    PRIMARY KEY (cart_id, item_id)
);
