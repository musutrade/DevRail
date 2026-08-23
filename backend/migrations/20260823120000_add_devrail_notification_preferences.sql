-- Per-user notification preferences. Push remains disabled until device registration is implemented.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:notification:write', '管理 DevRail 通知设置', 'api', '允许修改个人通知偏好。', 170)
ON CONFLICT (code) DO UPDATE SET name = EXCLUDED.name, type = EXCLUDED.type, description = EXCLUDED.description, sort_order = EXCLUDED.sort_order;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r JOIN permissions p ON p.code = 'devrail:notification:write'
WHERE r.code = 'super_admin' ON CONFLICT DO NOTHING;

CREATE TABLE devrail_notification_preferences (
    organization_id BIGINT NOT NULL REFERENCES organizations (id),
    user_id BIGINT NOT NULL REFERENCES users (id),
    in_app_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    push_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    event_types JSONB NOT NULL DEFAULT '["run.completed","run.failed","devrail.approval.requested","devrail.approval.approved","devrail.approval.rejected","devrail.approval.cancelled","devrail.approval.expired"]'::jsonb,
    quiet_hours JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id)
);
