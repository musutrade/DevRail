-- DevRail transactional outbox and in-app notification facts.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:notification:read', '查看 DevRail 通知', 'menu', '允许查看数据范围内的站内通知。', 160)
ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, type = EXCLUDED.type, description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code = 'devrail:notification:read'
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_notifications (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    department_id BIGINT,
    recipient_user_id BIGINT NOT NULL REFERENCES users (id),
    event_type VARCHAR(64) NOT NULL,
    level VARCHAR(16) NOT NULL CHECK (level IN ('info','success','warning','error','critical')),
    title VARCHAR(256) NOT NULL,
    summary TEXT NOT NULL,
    resource_type VARCHAR(64),
    resource_id BIGINT,
    deep_link VARCHAR(512),
    source_key VARCHAR(256) NOT NULL,
    read_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (recipient_user_id, source_key),
    FOREIGN KEY (department_id, organization_id) REFERENCES departments (id, organization_id)
);
CREATE INDEX idx_devrail_notifications_recipient ON devrail_notifications (recipient_user_id, read_at, created_at DESC);

CREATE TABLE devrail_outbox_events (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    event_type VARCHAR(64) NOT NULL,
    aggregate_type VARCHAR(64) NOT NULL,
    aggregate_id BIGINT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    status VARCHAR(16) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','processing','published','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (organization_id, event_type, aggregate_type, aggregate_id)
);
CREATE INDEX idx_devrail_outbox_pending ON devrail_outbox_events (status, available_at, id);
