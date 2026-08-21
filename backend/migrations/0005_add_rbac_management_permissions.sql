-- Permissions required by the API authorization layer.
INSERT INTO permissions (group_id, code, name, type, description, sort_order)
VALUES
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'role:directory:read', 'View Roles', 'menu', 'View role definitions and membership counts.', 5),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'role:write', 'Manage Roles', 'api', 'Create, update, and delete role definitions.', 6),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'role:permissions:write', 'Assign Role Permissions', 'api', 'Change permissions assigned to roles.', 7),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'permission:directory:read', 'View Permissions', 'menu', 'View the permission catalog.', 8);

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'role:directory:read',
    'role:write',
    'role:permissions:write',
    'permission:directory:read'
)
WHERE r.code = 'super_admin';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('role:directory:read', 'permission:directory:read')
WHERE r.code IN ('editor', 'viewer', 'compliance_auditor', 'support_tier2', 'billing_manager');
