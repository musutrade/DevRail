-- 将系统预置文案统一为简体中文。
-- 每个字段仅在仍等于原始英文默认值时更新，避免覆盖管理员的定制内容。

UPDATE permission_groups
SET name = CASE WHEN name = 'Dashboard Module' THEN '仪表盘模块' ELSE name END
WHERE code = 'dashboard';

UPDATE permission_groups
SET name = CASE WHEN name = 'Identity & Access Module' THEN '身份与访问模块' ELSE name END
WHERE code = 'identity';

UPDATE permissions
SET name = CASE WHEN name = 'View Analytics' THEN '查看统计分析' ELSE name END,
    description = CASE
        WHEN description = 'Ability to access and view the statistical charts in dashboard.'
            THEN '允许访问并查看仪表盘中的统计图表。'
        ELSE description
    END
WHERE code = 'dashboard:analytics:read';

UPDATE permissions
SET name = CASE WHEN name = 'User Write Access' THEN '管理用户' ELSE name END,
    description = CASE
        WHEN description = 'Allows creating and updating user account data via REST endpoints.'
            THEN '允许通过系统接口创建和更新用户账号。'
        ELSE description
    END
WHERE code = 'user:write';

UPDATE permissions
SET name = CASE WHEN name = 'Password Reset Trigger' THEN '重置用户密码' ELSE name END,
    description = CASE
        WHEN description = 'Grants authority to force a password reset on any user account.'
            THEN '允许强制重置任意用户账号的密码。'
        ELSE description
    END
WHERE code = 'user:admin:reset_password';

UPDATE permissions
SET name = CASE WHEN name = 'View Directory' THEN '查看用户目录' ELSE name END,
    description = CASE
        WHEN description = 'Browse the full employee directory and contact details.'
            THEN '允许浏览完整的用户目录和联系信息。'
        ELSE description
    END
WHERE code = 'user:directory:read';

UPDATE permissions
SET name = CASE WHEN name = 'Deactivate Accounts' THEN '停用用户账号' ELSE name END,
    description = CASE
        WHEN description = 'Suspend or deactivate user accounts across the organization.'
            THEN '允许暂停或停用组织内的用户账号。'
        ELSE description
    END
WHERE code = 'user:admin:deactivate';

UPDATE permissions
SET name = CASE WHEN name = 'View Roles' THEN '查看角色' ELSE name END,
    description = CASE
        WHEN description = 'View role definitions and membership counts.'
            THEN '允许查看角色定义及成员数量。'
        ELSE description
    END
WHERE code = 'role:directory:read';

UPDATE permissions
SET name = CASE WHEN name = 'Manage Roles' THEN '管理角色' ELSE name END,
    description = CASE
        WHEN description = 'Create, update, and delete role definitions.'
            THEN '允许创建、更新和删除角色定义。'
        ELSE description
    END
WHERE code = 'role:write';

UPDATE permissions
SET name = CASE WHEN name = 'Assign Role Permissions' THEN '分配角色权限' ELSE name END,
    description = CASE
        WHEN description = 'Change permissions assigned to roles.'
            THEN '允许修改分配给角色的权限。'
        ELSE description
    END
WHERE code = 'role:permissions:write';

UPDATE permissions
SET name = CASE WHEN name = 'View Permissions' THEN '查看权限' ELSE name END,
    description = CASE
        WHEN description = 'View the permission catalog.'
            THEN '允许查看权限目录。'
        ELSE description
    END
WHERE code = 'permission:directory:read';

UPDATE roles
SET name = CASE WHEN name = 'Super Admin' THEN '超级管理员' ELSE name END,
    category = CASE WHEN category = 'System Core' THEN '系统核心' ELSE category END,
    description = CASE
        WHEN description = 'Full access to all modules, including billing, user management, and global security settings.'
            THEN '拥有所有模块的完整访问权限，包括账单、用户管理和全局安全设置。'
        ELSE description
    END
WHERE code = 'super_admin';

UPDATE roles
SET name = CASE WHEN name = 'Editor' THEN '编辑者' ELSE name END,
    category = CASE WHEN category = 'Content' THEN '内容管理' ELSE category END,
    description = CASE
        WHEN description = 'Can create, edit, and publish content. No access to financial or security settings.'
            THEN '可以创建、编辑和发布内容，但不能访问财务或安全设置。'
        ELSE description
    END
WHERE code = 'editor';

UPDATE roles
SET name = CASE WHEN name = 'Viewer' THEN '查看者' ELSE name END,
    category = CASE WHEN category = 'Read Only' THEN '只读' ELSE category END,
    description = CASE
        WHEN description = 'Can view data, reports, and dashboards. No modification rights across the system.'
            THEN '可以查看数据、报表和仪表盘，但不能修改系统内容。'
        ELSE description
    END
WHERE code = 'viewer';

UPDATE roles
SET name = CASE WHEN name = 'Compliance Auditor' THEN '合规审计员' ELSE name END,
    category = CASE WHEN category = 'Compliance' THEN '合规管理' ELSE category END,
    description = CASE
        WHEN description = 'Special access to audit logs and security reporting for quarterly reviews.'
            THEN '拥有审计日志和安全报告的专项访问权限，用于定期合规审查。'
        ELSE description
    END
WHERE code = 'compliance_auditor';

UPDATE roles
SET name = CASE WHEN name = 'Support Tier 2' THEN '二线支持' ELSE name END,
    category = CASE WHEN category = 'Service' THEN '服务支持' ELSE category END,
    description = CASE
        WHEN description = 'Helpdesk troubleshooting rights including user password resets and ticket escalation.'
            THEN '拥有服务台故障处理权限，包括重置用户密码和升级工单。'
        ELSE description
    END
WHERE code = 'support_tier2';

UPDATE roles
SET name = CASE WHEN name = 'Billing Manager' THEN '账单管理员' ELSE name END,
    category = CASE WHEN category = 'Finance' THEN '财务管理' ELSE category END,
    description = CASE
        WHEN description = 'Invoice management, payment processing, and subscription plan adjustments.'
            THEN '可以管理发票、处理付款并调整订阅方案。'
        ELSE description
    END
WHERE code = 'billing_manager';
