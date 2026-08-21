INSERT INTO permission_groups (code, name, icon, sort_order)
VALUES ('organization', '组织管理', 'account_tree', 100)
ON CONFLICT (code) DO UPDATE
SET name = EXCLUDED.name,
    icon = EXCLUDED.icon;

INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'organization'),
     'organization:department:read',
     '查看部门',
     'menu',
     '允许查看组织内的部门结构和成员数量。',
     10),
    ((SELECT id FROM permission_groups WHERE code = 'organization'),
     'organization:department:write',
     '管理部门',
     'api',
     '允许创建、修改、移动和删除部门。',
     20)
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
    'organization:department:read',
    'organization:department:write'
)
WHERE r.code = 'super_admin'
ON CONFLICT DO NOTHING;
