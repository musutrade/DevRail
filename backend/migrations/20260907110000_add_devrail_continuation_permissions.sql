-- Continuation permissions use separate actions because reading lineage,
-- creating a request, and cancelling before dispatch have different risks.

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:continuation:read', '查看继续执行', 'api',
     '允许查看数据范围内 continuation 请求及运行谱系。', 150),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:continuation:create', '创建继续执行', 'api',
     '允许为数据范围内的终态任务追加受控上下文。', 151),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'),
     'devrail:continuation:cancel', '取消继续执行', 'api',
     '允许在 Agent 启动前取消数据范围内的 continuation 请求。', 152)
ON CONFLICT (code) DO UPDATE
SET group_id = EXCLUDED.group_id,
    name = EXCLUDED.name,
    type = EXCLUDED.type,
    description = EXCLUDED.description,
    sort_order = EXCLUDED.sort_order;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'devrail:continuation:read',
    'devrail:continuation:create',
    'devrail:continuation:cancel'
)
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:continuation:read'
WHERE r.code IN ('editor', 'viewer', 'compliance_auditor', 'support_tier2', 'billing_manager')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'devrail:continuation:create',
    'devrail:continuation:cancel'
)
WHERE r.code = 'editor'
ON CONFLICT DO NOTHING;
