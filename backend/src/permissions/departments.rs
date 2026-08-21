use crate::auth::PermissionRequirement;

pub const READ_PERMISSION_CODE: &str = "organization:department:read";
pub const WRITE_PERMISSION_CODE: &str = "organization:department:write";

pub struct DepartmentRead;

impl PermissionRequirement for DepartmentRead {
    const CODE: &'static str = READ_PERMISSION_CODE;
}

pub struct DepartmentWrite;

impl PermissionRequirement for DepartmentWrite {
    const CODE: &'static str = WRITE_PERMISSION_CODE;
}
