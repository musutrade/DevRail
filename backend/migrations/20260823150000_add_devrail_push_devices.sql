INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:push_device:read', '查看推送设备', 'api', '允许查看个人推送设备。', 180),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:push_device:write', '注册推送设备', 'api', '允许注册个人推送设备。', 190),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:push_device:revoke', '撤销推送设备', 'api', '允许撤销个人推送设备。', 200)
ON CONFLICT (code) DO UPDATE SET name=EXCLUDED.name, description=EXCLUDED.description, sort_order=EXCLUDED.sort_order;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code LIKE 'devrail:push_device:%'
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_push_devices (
    id BIGSERIAL PRIMARY KEY,
    organization_id BIGINT NOT NULL REFERENCES organizations(id),
    user_id BIGINT NOT NULL REFERENCES users(id),
    device_name VARCHAR(128) NOT NULL,
    platform VARCHAR(32) NOT NULL,
    browser VARCHAR(64),
    timezone VARCHAR(64),
    client_version VARCHAR(64),
    endpoint_ciphertext BYTEA NOT NULL,
    endpoint_fingerprint CHAR(64) NOT NULL,
    keys_ciphertext BYTEA NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'active' CHECK (status IN ('active','revoked','invalid')),
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (organization_id, user_id, endpoint_fingerprint)
);
CREATE INDEX idx_devrail_push_devices_user ON devrail_push_devices (organization_id, user_id, status, updated_at DESC);
