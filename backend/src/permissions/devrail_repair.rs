use crate::auth::PermissionRequirement;

pub struct RepairRead;
impl PermissionRequirement for RepairRead {
    const CODE: &'static str = "devrail:repair:read";
}

pub struct RepairCreate;
impl PermissionRequirement for RepairCreate {
    const CODE: &'static str = "devrail:repair:create";
}

pub struct RepairCancel;
impl PermissionRequirement for RepairCancel {
    const CODE: &'static str = "devrail:repair:cancel";
}

pub struct RepairApprove;
impl PermissionRequirement for RepairApprove {
    const CODE: &'static str = "devrail:repair:approve";
}

pub struct RepairHandoff;
impl PermissionRequirement for RepairHandoff {
    const CODE: &'static str = "devrail:repair:handoff";
}
