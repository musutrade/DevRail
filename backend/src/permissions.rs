//! 类型化权限标记。平台权限保留在本文件，业务权限按模块放入 `permissions/`。

use crate::auth::PermissionRequirement;

pub mod departments;

macro_rules! define_permission {
    ($name:ident, $code:literal) => {
        pub struct $name;

        impl PermissionRequirement for $name {
            const CODE: &'static str = $code;
        }
    };
}

define_permission!(UserRead, "user:directory:read");
define_permission!(UserWrite, "user:write");
define_permission!(UserRoleWrite, "user:roles:write");
define_permission!(UserDeactivate, "user:admin:deactivate");
define_permission!(RoleRead, "role:directory:read");
define_permission!(RoleWrite, "role:write");
define_permission!(RolePermissionWrite, "role:permissions:write");
define_permission!(PermissionRead, "permission:directory:read");
define_permission!(DashboardRead, "dashboard:analytics:read");
define_permission!(AuditLogRead, "audit:logs:read");
