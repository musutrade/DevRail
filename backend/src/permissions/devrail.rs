use crate::auth::PermissionRequirement;

pub struct ProjectRead;
impl PermissionRequirement for ProjectRead {
    const CODE: &'static str = "devrail:project:read";
}
pub struct ProjectWrite;
impl PermissionRequirement for ProjectWrite {
    const CODE: &'static str = "devrail:project:write";
}
pub struct RepositoryRead;
impl PermissionRequirement for RepositoryRead {
    const CODE: &'static str = "devrail:repository:read";
}
pub struct RepositoryWrite;
impl PermissionRequirement for RepositoryWrite {
    const CODE: &'static str = "devrail:repository:write";
}
pub struct EnvironmentRead;
impl PermissionRequirement for EnvironmentRead {
    const CODE: &'static str = "devrail:environment:read";
}
pub struct EnvironmentWrite;
impl PermissionRequirement for EnvironmentWrite {
    const CODE: &'static str = "devrail:environment:write";
}
pub struct TaskRead;
impl PermissionRequirement for TaskRead {
    const CODE: &'static str = "devrail:task:read";
}
pub struct TaskWrite;
impl PermissionRequirement for TaskWrite {
    const CODE: &'static str = "devrail:task:write";
}
pub struct MemberRead;
impl PermissionRequirement for MemberRead {
    const CODE: &'static str = "devrail:member:read";
}
pub struct MemberWrite;
impl PermissionRequirement for MemberWrite {
    const CODE: &'static str = "devrail:member:write";
}
pub struct RunRead;
impl PermissionRequirement for RunRead {
    const CODE: &'static str = "devrail:run:read";
}
pub struct RunExecute;
impl PermissionRequirement for RunExecute {
    const CODE: &'static str = "devrail:run:execute";
}
pub struct RunInterrupt;
impl PermissionRequirement for RunInterrupt {
    const CODE: &'static str = "devrail:run:interrupt";
}
pub struct RunRetry;
impl PermissionRequirement for RunRetry {
    const CODE: &'static str = "devrail:run:retry";
}
pub struct ApprovalRead;
impl PermissionRequirement for ApprovalRead {
    const CODE: &'static str = "devrail:approval:read";
}
pub struct ApprovalApprove;
impl PermissionRequirement for ApprovalApprove {
    const CODE: &'static str = "devrail:approval:approve";
}
pub struct ApprovalReject;
impl PermissionRequirement for ApprovalReject {
    const CODE: &'static str = "devrail:approval:reject";
}
pub struct NotificationRead;
impl PermissionRequirement for NotificationRead {
    const CODE: &'static str = "devrail:notification:read";
}
pub struct NotificationWrite;
impl PermissionRequirement for NotificationWrite {
    const CODE: &'static str = "devrail:notification:write";
}
