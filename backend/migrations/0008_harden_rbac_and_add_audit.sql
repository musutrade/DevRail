-- Harden privileged grants, support token revocation, and persist RBAC audit events.

ALTER TABLE users
ADD COLUMN token_version BIGINT NOT NULL DEFAULT 0;

-- Historical soft-deleted users may still have role bindings from earlier releases.
DELETE FROM user_roles ur
USING users u
WHERE u.id = ur.user_id
  AND u.deleted_at IS NOT NULL;

CREATE TABLE audit_logs (
    id            BIGSERIAL PRIMARY KEY,
    actor_user_id BIGINT REFERENCES users (id) ON DELETE SET NULL,
    action        VARCHAR(96)  NOT NULL,
    target_type   VARCHAR(64)  NOT NULL,
    target_id     BIGINT,
    details       JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_logs_created_at ON audit_logs (created_at DESC, id DESC);
CREATE INDEX idx_audit_logs_actor ON audit_logs (actor_user_id, created_at DESC);
CREATE INDEX idx_audit_logs_action ON audit_logs (action, created_at DESC);

INSERT INTO permission_groups (code, name, icon, sort_order)
VALUES ('audit', '审计与合规模块', 'fact_check', 3)
ON CONFLICT (code) DO UPDATE
SET name = EXCLUDED.name,
    icon = EXCLUDED.icon,
    sort_order = EXCLUDED.sort_order;

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:roles:write', '分配用户角色', 'api', '允许修改用户与普通角色之间的分配关系。', 9),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:super_admin:grant', '授予超级管理员', 'button', '允许向用户授予内置超级管理员角色。', 10),
    ((SELECT id FROM permission_groups WHERE code = 'audit'),
     'audit:logs:read', '查看审计日志', 'menu', '允许查看角色、权限和账号安全变更记录。', 1)
ON CONFLICT (code) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'user:roles:write',
    'user:super_admin:grant',
    'audit:logs:read'
)
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'audit:logs:read'
WHERE r.code = 'compliance_auditor'
ON CONFLICT DO NOTHING;
