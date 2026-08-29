-- Repair permissions are separate from run retry and continuation actions.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repair:read', '查看受控修复', 'menu', '允许查看数据范围内的修复请求、诊断和修复谱系。', 160),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repair:create', '创建受控修复', 'api', '允许为可信失败证据创建受策略约束的修复请求。', 161),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repair:cancel', '取消受控修复', 'api', '允许在 Agent 启动前取消修复请求。', 162),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repair:approve', '审批受控修复', 'api', '允许审批需要人工确认的修复操作。', 163),
    ((SELECT id FROM permission_groups WHERE code = 'devrail'), 'devrail:repair:handoff', '处理修复交接', 'api', '允许处理人工交接、人工重试和最终修复结论。', 164)
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
    'devrail:repair:read', 'devrail:repair:create', 'devrail:repair:cancel',
    'devrail:repair:approve', 'devrail:repair:handoff'
)
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'devrail:repair:read'
WHERE r.code IN ('editor', 'viewer', 'compliance_auditor', 'support_tier2')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('devrail:repair:create', 'devrail:repair:cancel')
WHERE r.code = 'editor'
ON CONFLICT DO NOTHING;
