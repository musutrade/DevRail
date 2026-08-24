CREATE TABLE devrail_push_deliveries (
    id BIGSERIAL PRIMARY KEY,
    outbox_event_id BIGINT NOT NULL REFERENCES devrail_outbox_events(id),
    push_device_id BIGINT NOT NULL REFERENCES devrail_push_devices(id),
    status VARCHAR(16) NOT NULL CHECK (status IN ('pending','sent','retrying','failed','invalid')),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (outbox_event_id, push_device_id)
);
CREATE INDEX idx_devrail_push_deliveries_pending ON devrail_push_deliveries(status, available_at, id);
