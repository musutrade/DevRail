-- 种子数据：5 个权限组 / 15 个权限 / 6 个角色 / 角色-权限分配
-- 与前端 mock-data.ts 保持一致（权限码、角色名称、分配关系）
-- 用户种子（含密码哈希）在实现认证时单独提供，避免明文/伪哈希入库

INSERT INTO permission_groups (code, name, icon, sort_order) VALUES
    ('dashboard', 'Dashboard Module',       'dashboard',    1),
    ('identity',  'Identity & Access Module', 'group',      2),
    ('resources', 'Resources Module',       'inventory_2',  3),
    ('audit',     'Audit & Compliance Module', 'fact_check', 4),
    ('security',  'Security Center Module', 'shield',       5);

INSERT INTO permissions (group_id, code, name, type, description, sort_order) VALUES
    ((SELECT id FROM permission_groups WHERE code = 'dashboard'),
     'dashboard:analytics:read',   'View Analytics',        'menu',   'Ability to access and view the statistical charts in dashboard.', 1),
    ((SELECT id FROM permission_groups WHERE code = 'dashboard'),
     'dashboard:analytics:export', 'Export Reports',        'button', 'Allows downloading CSV/PDF versions of analytics reports.', 2),
    ((SELECT id FROM permission_groups WHERE code = 'dashboard'),
     'dashboard:widgets:manage',   'Configure Widgets',     'api',    'Persist dashboard widget layout and data-source bindings.', 3),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:write',                 'User Write Access',     'api',    'Allows creating and updating user account data via REST endpoints.', 1),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:admin:reset_password',  'Password Reset Trigger','button', 'Grants authority to force a password reset on any user account.', 2),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:directory:read',        'View Directory',        'menu',   'Browse the full employee directory and contact details.', 3),
    ((SELECT id FROM permission_groups WHERE code = 'identity'),
     'user:admin:deactivate',      'Deactivate Accounts',   'button', 'Suspend or deactivate user accounts across the organization.', 4),
    ((SELECT id FROM permission_groups WHERE code = 'resources'),
     'resource:hw:read',           'Hardware Inventory Read','menu',  'View the list of all hardware assets in the system.', 1),
    ((SELECT id FROM permission_groups WHERE code = 'resources'),
     'resource:infra:manage',      'Modify Infrastructure', 'api',    'Critical permission for modifying cloud infrastructure components.', 2),
    ((SELECT id FROM permission_groups WHERE code = 'resources'),
     'resource:license:grant',     'License Provisioning',  'button', 'Grant or revoke software license seats for teams.', 3),
    ((SELECT id FROM permission_groups WHERE code = 'audit'),
     'audit:logs:read',            'View Audit Logs',       'menu',   'Read immutable audit trail for all admin actions.', 1),
    ((SELECT id FROM permission_groups WHERE code = 'audit'),
     'audit:logs:export',          'Export Audit Evidence', 'button', 'Export signed audit evidence for external compliance reviews.', 2),
    ((SELECT id FROM permission_groups WHERE code = 'security'),
     'security:policies:manage',   'Manage Policies',       'api',    'Create and update global security policies and rule sets.', 1),
    ((SELECT id FROM permission_groups WHERE code = 'security'),
     'security:mfa:enforce',       'MFA Enforcement',       'button', 'Force multi-factor authentication enrollment for user groups.', 2),
    ((SELECT id FROM permission_groups WHERE code = 'security'),
     'security:threats:read',      'View Threat Events',    'menu',   'Access real-time threat detection and anomaly events.', 3);

INSERT INTO roles (code, name, category, icon, color, description) VALUES
    ('super_admin',        'Super Admin',        'System Core', 'rocket_launch',         'primary', 'Full access to all modules, including billing, user management, and global security settings.'),
    ('editor',             'Editor',             'Content',     'edit_note',             'warning', 'Can create, edit, and publish content. No access to financial or security settings.'),
    ('viewer',             'Viewer',             'Read Only',   'visibility',            'success', 'Can view data, reports, and dashboards. No modification rights across the system.'),
    ('compliance_auditor', 'Compliance Auditor', 'Compliance',  'fact_check',            'danger',  'Special access to audit logs and security reporting for quarterly reviews.'),
    ('support_tier2',      'Support Tier 2',     'Service',     'support_agent',         'neutral', 'Helpdesk troubleshooting rights including user password resets and ticket escalation.'),
    ('billing_manager',    'Billing Manager',    'Finance',     'account_balance_wallet','warning', 'Invoice management, payment processing, and subscription plan adjustments.');

-- 角色-权限分配（与 mock 的 MOCK_ASSIGNED_MAP 一致）
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'dashboard:analytics:read', 'dashboard:analytics:export', 'dashboard:widgets:manage',
    'user:write', 'user:admin:reset_password', 'user:directory:read', 'user:admin:deactivate',
    'resource:hw:read', 'resource:infra:manage', 'resource:license:grant',
    'audit:logs:read', 'audit:logs:export',
    'security:policies:manage', 'security:mfa:enforce', 'security:threats:read'
)
WHERE r.code = 'super_admin';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('audit:logs:read', 'audit:logs:export', 'security:policies:manage', 'security:threats:read')
WHERE r.code = 'compliance_auditor';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('dashboard:analytics:read', 'resource:hw:read')
WHERE r.code = 'viewer';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('dashboard:analytics:read', 'dashboard:analytics:export')
WHERE r.code = 'editor';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('user:write', 'user:admin:reset_password', 'user:directory:read')
WHERE r.code = 'support_tier2';

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'resource:hw:read', 'resource:infra:manage', 'resource:license:grant',
    'dashboard:analytics:read', 'dashboard:analytics:export', 'dashboard:widgets:manage'
)
WHERE r.code = 'billing_manager';
