use crate::auth::PermissionRequirement;

pub struct ContinuationRead;

impl PermissionRequirement for ContinuationRead {
    const CODE: &'static str = "devrail:continuation:read";
}

pub struct ContinuationCreate;

impl PermissionRequirement for ContinuationCreate {
    const CODE: &'static str = "devrail:continuation:create";
}

pub struct ContinuationCancel;

impl PermissionRequirement for ContinuationCancel {
    const CODE: &'static str = "devrail:continuation:cancel";
}
