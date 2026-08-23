//! OpenAPI contract generated from Rust DTOs and operation declarations.
#![expect(
    dead_code,
    reason = "Utoipa operation declarations are consumed by the OpenApi derive macro"
)]

use crate::error::ErrorEnvelope;
use crate::models::{
    AddDevRailProjectMemberRequest, AssignRolesRequest, AuditLogQuery, BatchAssignRolesRequest,
    BatchUserIdsRequest, ChangePasswordRequest, CreateDepartmentRequest,
    CreateDevRailEnvironmentRequest, CreateDevRailProjectRequest, CreateDevRailRepositoryRequest,
    CreateDevRailRunRequest, CreateDevRailTaskRequest, CreateRoleRequest, CreateUserRequest,
    DashboardStats, DataScopeSchema, DepartmentResponse, DepartmentStatusSchema,
    DevRailApprovalDecisionRequest, DevRailApprovalPage, DevRailApprovalResponse,
    DevRailChangesetResponse, DevRailEnvironmentPage, DevRailEnvironmentResponse, DevRailListQuery,
    DevRailProjectMemberPage, DevRailProjectMemberResponse, DevRailProjectPage,
    DevRailProjectPolicyResponse, DevRailProjectResponse, DevRailQualityGatePage,
    DevRailRepositoryPage, DevRailRepositoryResponse, DevRailRunEventPage, DevRailRunPage,
    DevRailRunResponse, DevRailTaskPage, DevRailTaskResponse, HealthResponse, LoginRequest,
    LoginResponse, LoginStatusSchema, MfaCodeRequest, MfaFactorRevokeRequest, MfaMethodSchema,
    MfaPasskeyAuthenticationFinishRequest, MfaPasskeyAuthenticationStartRequest,
    MfaPasskeyRegistrationFinishRequest, MfaPasskeyRegistrationStartRequest, MfaPasskeyResponse,
    MfaStatusResponse, MfaWebauthnChallengeResponse, ModuleUnlockRequest, ModuleUnlockScopeSchema,
    ModuleUnlockStatusResponse, PageAuditLog, PageQuery, PageUser, PermissionCodes,
    PermissionGroupResponse, PermissionResponse, PermissionTypeSchema, ReadinessResponse,
    RecoveryCodesResponse, RetryDevRailRunRequest, RoleColorSchema, RolePermissions, RoleResponse,
    SortDirectionSchema, StepUpRequest, StepUpResponse, UpdateDepartmentRequest,
    UpdateDevRailEnvironmentRequest, UpdateDevRailProjectPolicyRequest,
    UpdateDevRailProjectRequest, UpdateDevRailRepositoryRequest, UpdateDevRailTaskRequest,
    UpdateRolePermissionsRequest, UpdateRoleRequest, UpdateUserRequest, UserResponse,
    UserSortBySchema, UserStatusSchema,
};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::openapi::OpenApi as OpenApiDocument;
use utoipa::{Modify, OpenApi};

const COOKIE_SECURITY: &str = "cookieAuth";

#[utoipa::path(
    get,
    path = "/healthz",
    operation_id = "healthCheck",
    tag = "system",
    responses((status = 200, description = "进程存活", body = HealthResponse))
)]
fn health_check() {}

#[utoipa::path(
    get,
    path = "/readyz",
    operation_id = "readinessCheck",
    tag = "system",
    responses(
        (status = 200, description = "服务就绪", body = ReadinessResponse),
        (status = 503, description = "依赖不可用", body = ReadinessResponse)
    )
)]
fn readiness_check() {}

#[utoipa::path(
    post,
    path = "/auth/login",
    operation_id = "login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "登录成功", body = LoginResponse),
        (status = 401, description = "凭据错误", body = ErrorEnvelope),
        (status = 429, description = "登录尝试过于频繁", body = ErrorEnvelope)
    )
)]
fn login() {}

#[utoipa::path(
    post,
    path = "/auth/logout",
    operation_id = "logout",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    responses(
        (status = 204, description = "退出成功"),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 403, description = "CSRF 校验失败", body = ErrorEnvelope)
    )
)]
fn logout() {}

#[utoipa::path(
    get,
    path = "/auth/me",
    operation_id = "getCurrentUser",
    tag = "auth",
    security(("cookieAuth" = [])),
    responses(
        (status = 200, description = "当前用户", body = UserResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope)
    )
)]
fn current_user() {}

#[utoipa::path(
    put,
    path = "/auth/me/password",
    operation_id = "changeCurrentUserPassword",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "密码修改再认证凭据")
    ),
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "密码已修改，现有会话已撤销"),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 422, description = "密码不符合要求", body = ErrorEnvelope)
    )
)]
fn change_current_user_password() {}

#[utoipa::path(
    post,
    path = "/auth/me/step-up",
    operation_id = "issueStepUpToken",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    request_body = StepUpRequest,
    responses(
        (status = 200, description = "再认证凭据已签发", body = StepUpResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 403, description = "该操作需要身份验证器验证码", body = ErrorEnvelope),
        (status = 422, description = "当前密码、验证码或操作范围无效", body = ErrorEnvelope)
    )
)]
fn issue_step_up_token() {}

#[utoipa::path(
    post,
    path = "/auth/me/module-unlocks",
    operation_id = "unlockCurrentUserModule",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    request_body = ModuleUnlockRequest,
    responses(
        (status = 200, description = "模块已临时解锁", body = ModuleUnlockStatusResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 403, description = "该操作需要身份验证器验证码", body = ErrorEnvelope),
        (status = 422, description = "当前密码、验证码或模块范围无效", body = ErrorEnvelope)
    )
)]
fn unlock_current_user_module() {}

#[utoipa::path(
    get,
    path = "/auth/me/module-unlocks/{module}",
    operation_id = "getCurrentUserModuleUnlockStatus",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("module" = ModuleUnlockScopeSchema, Path, description = "模块范围")),
    responses(
        (status = 200, description = "模块解锁状态", body = ModuleUnlockStatusResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 422, description = "模块范围无效", body = ErrorEnvelope)
    )
)]
fn current_user_module_unlock_status() {}

#[utoipa::path(
    get,
    path = "/auth/me/permissions",
    operation_id = "getCurrentUserPermissions",
    tag = "auth",
    security(("cookieAuth" = [])),
    responses(
        (status = 200, description = "有效权限码", body = PermissionCodes),
        (status = 401, description = "未认证", body = ErrorEnvelope)
    )
)]
fn current_user_permissions() {}

#[utoipa::path(
    post,
    path = "/auth/mfa/totp/verify",
    operation_id = "verifyMfaTotp",
    tag = "auth",
    request_body = MfaCodeRequest,
    responses(
        (status = 200, description = "TOTP 验证成功并创建完整会话", body = LoginResponse),
        (status = 401, description = "验证码无效", body = ErrorEnvelope),
        (status = 429, description = "二次验证尝试过于频繁", body = ErrorEnvelope)
    )
)]
fn verify_mfa_totp() {}

#[utoipa::path(
    post,
    path = "/auth/mfa/recovery/verify",
    operation_id = "verifyMfaRecoveryCode",
    tag = "auth",
    request_body = MfaCodeRequest,
    responses(
        (status = 200, description = "恢复码验证成功并创建完整会话", body = LoginResponse),
        (status = 401, description = "恢复码无效", body = ErrorEnvelope),
        (status = 429, description = "二次验证尝试过于频繁", body = ErrorEnvelope)
    )
)]
fn verify_mfa_recovery_code() {}

#[utoipa::path(
    post,
    path = "/auth/mfa/passkey/authenticate/start",
    operation_id = "startMfaPasskeyAuthentication",
    tag = "auth",
    request_body = MfaPasskeyAuthenticationStartRequest,
    responses(
        (status = 200, description = "通行密钥认证挑战", body = MfaWebauthnChallengeResponse),
        (status = 401, description = "挑战无效", body = ErrorEnvelope)
    )
)]
fn start_mfa_passkey_authentication() {}

#[utoipa::path(
    post,
    path = "/auth/mfa/passkey/authenticate/finish",
    operation_id = "finishMfaPasskeyAuthentication",
    tag = "auth",
    request_body = MfaPasskeyAuthenticationFinishRequest,
    responses(
        (status = 200, description = "通行密钥验证成功并创建完整会话", body = LoginResponse),
        (status = 401, description = "通行密钥验证失败", body = ErrorEnvelope)
    )
)]
fn finish_mfa_passkey_authentication() {}

#[utoipa::path(
    get,
    path = "/auth/me/mfa",
    operation_id = "getCurrentUserMfaStatus",
    tag = "auth",
    security(("cookieAuth" = [])),
    responses(
        (status = 200, description = "当前用户 MFA 状态", body = MfaStatusResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope)
    )
)]
fn current_user_mfa_status() {}

#[utoipa::path(
    post,
    path = "/auth/me/mfa/passkey/register/start",
    operation_id = "startCurrentUserPasskeyRegistration",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    request_body = MfaPasskeyRegistrationStartRequest,
    responses(
        (status = 200, description = "通行密钥注册挑战", body = MfaWebauthnChallengeResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 422, description = "重新认证失败", body = ErrorEnvelope)
    )
)]
fn start_current_user_passkey_registration() {}

#[utoipa::path(
    post,
    path = "/auth/me/mfa/passkey/register/finish",
    operation_id = "finishCurrentUserPasskeyRegistration",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    request_body = MfaPasskeyRegistrationFinishRequest,
    responses(
        (status = 200, description = "通行密钥已注册", body = MfaStatusResponse),
        (status = 422, description = "通行密钥响应无效", body = ErrorEnvelope)
    )
)]
fn finish_current_user_passkey_registration() {}

#[utoipa::path(
    delete,
    path = "/auth/me/mfa/passkey/{id}",
    operation_id = "revokeCurrentUserPasskey",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "通行密钥 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")
    ),
    request_body = MfaFactorRevokeRequest,
    responses(
        (status = 204, description = "通行密钥已撤销，全部会话已撤销"),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 422, description = "重新认证失败", body = ErrorEnvelope)
    )
)]
fn revoke_current_user_passkey() {}

#[utoipa::path(
    post,
    path = "/auth/me/mfa/recovery-codes",
    operation_id = "regenerateCurrentUserRecoveryCodes",
    tag = "auth",
    security(("cookieAuth" = [])),
    params(("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token")),
    request_body = MfaFactorRevokeRequest,
    responses(
        (status = 200, description = "恢复码已重新生成，全部会话已撤销", body = RecoveryCodesResponse),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 422, description = "重新认证失败", body = ErrorEnvelope)
    )
)]
fn regenerate_current_user_recovery_codes() {}

#[utoipa::path(
    get,
    path = "/users",
    operation_id = "listUsers",
    tag = "users",
    security(("cookieAuth" = [])),
    params(PageQuery),
    responses(
        (status = 200, description = "用户分页结果", body = PageUser),
        (status = 401, description = "未认证", body = ErrorEnvelope),
        (status = 403, description = "无权读取用户", body = ErrorEnvelope),
        (status = 422, description = "查询参数无效", body = ErrorEnvelope)
    )
)]
fn list_users() {}

#[utoipa::path(
    post,
    path = "/users",
    operation_id = "createUser",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = Option<String>, Header, description = "敏感操作再认证凭据")
    ),
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "用户已创建", body = UserResponse),
        (status = 403, description = "无权创建用户", body = ErrorEnvelope),
        (status = 409, description = "用户名冲突", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn create_user() {}

#[utoipa::path(
    get,
    path = "/users/{id}",
    operation_id = "getUser",
    tag = "users",
    security(("cookieAuth" = [])),
    params(("id" = i64, Path, description = "用户 ID")),
    responses(
        (status = 200, description = "用户详情", body = UserResponse),
        (status = 404, description = "用户不存在或超出数据范围", body = ErrorEnvelope)
    )
)]
fn get_user() {}

#[utoipa::path(
    put,
    path = "/users/{id}",
    operation_id = "updateUser",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "用户 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = Option<String>, Header, description = "敏感操作再认证凭据")
    ),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "用户已更新", body = UserResponse),
        (status = 403, description = "无权更新用户", body = ErrorEnvelope),
        (status = 404, description = "用户不存在或超出数据范围", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn update_user() {}

#[utoipa::path(
    delete,
    path = "/users/{id}",
    operation_id = "deleteUser",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "用户 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "用户删除再认证凭据")
    ),
    responses(
        (status = 204, description = "用户已删除"),
        (status = 403, description = "无权删除用户", body = ErrorEnvelope),
        (status = 404, description = "用户不存在或超出数据范围", body = ErrorEnvelope)
    )
)]
fn delete_user() {}

#[utoipa::path(
    post,
    path = "/users/batch-delete",
    operation_id = "batchDeleteUsers",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "用户删除再认证凭据")
    ),
    request_body = BatchUserIdsRequest,
    responses(
        (status = 204, description = "用户已批量删除"),
        (status = 403, description = "无权批量删除用户", body = ErrorEnvelope),
        (status = 422, description = "用户列表无效", body = ErrorEnvelope)
    )
)]
fn batch_delete_users() {}

#[utoipa::path(
    put,
    path = "/users/{id}/roles",
    operation_id = "assignUserRoles",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "用户 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "角色分配再认证凭据")
    ),
    request_body = AssignRolesRequest,
    responses(
        (status = 204, description = "角色已更新"),
        (status = 403, description = "无权分配角色", body = ErrorEnvelope),
        (status = 404, description = "用户不存在或超出数据范围", body = ErrorEnvelope),
        (status = 422, description = "角色无效", body = ErrorEnvelope)
    )
)]
fn assign_user_roles() {}

#[utoipa::path(
    put,
    path = "/users/batch-roles",
    operation_id = "batchAssignUserRoles",
    tag = "users",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "角色分配再认证凭据")
    ),
    request_body = BatchAssignRolesRequest,
    responses(
        (status = 204, description = "用户角色已批量更新"),
        (status = 403, description = "无权批量分配角色", body = ErrorEnvelope),
        (status = 422, description = "用户或角色列表无效", body = ErrorEnvelope)
    )
)]
fn batch_assign_user_roles() {}

#[utoipa::path(
    get,
    path = "/roles",
    operation_id = "listRoles",
    tag = "roles",
    security(("cookieAuth" = [])),
    responses((status = 200, description = "角色列表", body = [RoleResponse]))
)]
fn list_roles() {}

#[utoipa::path(
    post,
    path = "/roles",
    operation_id = "createRole",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = Option<String>, Header, description = "权限变更再认证凭据")
    ),
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "角色已创建", body = RoleResponse),
        (status = 403, description = "无权创建角色", body = ErrorEnvelope),
        (status = 409, description = "角色编码冲突", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn create_role() {}

#[utoipa::path(
    get,
    path = "/roles/{id}",
    operation_id = "getRole",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(("id" = i64, Path, description = "角色 ID")),
    responses(
        (status = 200, description = "角色详情", body = RoleResponse),
        (status = 404, description = "角色不存在", body = ErrorEnvelope)
    )
)]
fn get_role() {}

#[utoipa::path(
    put,
    path = "/roles/{id}",
    operation_id = "updateRole",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "角色 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = Option<String>, Header, description = "敏感操作再认证凭据")
    ),
    request_body = UpdateRoleRequest,
    responses(
        (status = 200, description = "角色已更新", body = RoleResponse),
        (status = 403, description = "无权更新角色", body = ErrorEnvelope),
        (status = 404, description = "角色不存在", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn update_role() {}

#[utoipa::path(
    delete,
    path = "/roles/{id}",
    operation_id = "deleteRole",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "角色 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "角色删除再认证凭据")
    ),
    responses(
        (status = 204, description = "角色已删除"),
        (status = 403, description = "无权删除角色", body = ErrorEnvelope),
        (status = 404, description = "角色不存在", body = ErrorEnvelope),
        (status = 409, description = "角色仍被使用", body = ErrorEnvelope)
    )
)]
fn delete_role() {}

#[utoipa::path(
    get,
    path = "/roles/{id}/permissions",
    operation_id = "getRolePermissions",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(("id" = i64, Path, description = "角色 ID")),
    responses(
        (status = 200, description = "角色权限", body = RolePermissions),
        (status = 404, description = "角色不存在", body = ErrorEnvelope)
    )
)]
fn get_role_permissions() {}

#[utoipa::path(
    put,
    path = "/roles/{id}/permissions",
    operation_id = "updateRolePermissions",
    tag = "roles",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "角色 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "权限分配再认证凭据")
    ),
    request_body = UpdateRolePermissionsRequest,
    responses(
        (status = 204, description = "角色权限已更新"),
        (status = 403, description = "无权分配权限", body = ErrorEnvelope),
        (status = 404, description = "角色不存在", body = ErrorEnvelope),
        (status = 422, description = "权限无效", body = ErrorEnvelope)
    )
)]
fn update_role_permissions() {}

#[utoipa::path(
    get,
    path = "/departments",
    operation_id = "listDepartments",
    tag = "departments",
    security(("cookieAuth" = [])),
    responses(
        (status = 200, description = "可见部门层级列表", body = [DepartmentResponse]),
        (status = 403, description = "无权查看部门", body = ErrorEnvelope)
    )
)]
fn list_departments() {}

#[utoipa::path(
    post,
    path = "/departments",
    operation_id = "createDepartment",
    tag = "departments",
    security(("cookieAuth" = [])),
    params(
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "部门管理再认证凭据")
    ),
    request_body = CreateDepartmentRequest,
    responses(
        (status = 201, description = "部门已创建", body = DepartmentResponse),
        (status = 403, description = "无权创建部门", body = ErrorEnvelope),
        (status = 409, description = "部门编码冲突", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn create_department() {}

#[utoipa::path(
    get,
    path = "/departments/{id}",
    operation_id = "getDepartment",
    tag = "departments",
    security(("cookieAuth" = [])),
    params(("id" = i64, Path, description = "部门 ID")),
    responses(
        (status = 200, description = "部门详情", body = DepartmentResponse),
        (status = 404, description = "部门不存在或不可见", body = ErrorEnvelope)
    )
)]
fn get_department() {}

#[utoipa::path(
    put,
    path = "/departments/{id}",
    operation_id = "updateDepartment",
    tag = "departments",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "部门 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "部门管理再认证凭据")
    ),
    request_body = UpdateDepartmentRequest,
    responses(
        (status = 200, description = "部门已更新", body = DepartmentResponse),
        (status = 403, description = "无权更新部门", body = ErrorEnvelope),
        (status = 404, description = "部门不存在或不可见", body = ErrorEnvelope),
        (status = 409, description = "部门编码冲突", body = ErrorEnvelope),
        (status = 422, description = "请求无效", body = ErrorEnvelope)
    )
)]
fn update_department() {}

#[utoipa::path(
    delete,
    path = "/departments/{id}",
    operation_id = "deleteDepartment",
    tag = "departments",
    security(("cookieAuth" = [])),
    params(
        ("id" = i64, Path, description = "部门 ID"),
        ("X-CSRF-Token" = String, Header, description = "当前会话的 CSRF Token"),
        ("X-Step-Up-Token" = String, Header, description = "部门删除再认证凭据")
    ),
    responses(
        (status = 204, description = "部门已删除"),
        (status = 403, description = "根部门不可删除", body = ErrorEnvelope),
        (status = 404, description = "部门不存在或不可见", body = ErrorEnvelope),
        (status = 409, description = "部门仍有成员或下级部门", body = ErrorEnvelope)
    )
)]
fn delete_department() {}

#[utoipa::path(
    get,
    path = "/permissions/groups",
    operation_id = "listPermissionGroups",
    tag = "permissions",
    security(("cookieAuth" = [])),
    responses((status = 200, description = "权限组树", body = [PermissionGroupResponse]))
)]
fn list_permission_groups() {}

#[utoipa::path(
    get,
    path = "/dashboard/stats",
    operation_id = "getDashboardStats",
    tag = "dashboard",
    security(("cookieAuth" = [])),
    responses((status = 200, description = "仪表盘统计", body = DashboardStats))
)]
fn dashboard_stats() {}

#[utoipa::path(
    get,
    path = "/audit-logs",
    operation_id = "listAuditLogs",
    tag = "audit",
    security(("cookieAuth" = [])),
    params(AuditLogQuery),
    responses(
        (status = 200, description = "审计日志分页结果", body = PageAuditLog),
        (status = 403, description = "无权读取审计日志", body = ErrorEnvelope)
    )
)]
fn list_audit_logs() {}

#[utoipa::path(get, path = "/projects", operation_id = "listDevRailProjects", tag = "devrail", security(("cookieAuth" = [])), params(DevRailListQuery), responses((status = 200, body = DevRailProjectPage), (status = 403, body = ErrorEnvelope)))]
fn list_devrail_projects() {}
#[utoipa::path(post, path = "/projects", operation_id = "createDevRailProject", tag = "devrail", security(("cookieAuth" = [])), request_body = CreateDevRailProjectRequest, responses((status = 201, body = DevRailProjectResponse), (status = 422, body = ErrorEnvelope)))]
fn create_devrail_project() {}
#[utoipa::path(get, path = "/projects/{id}", operation_id = "getDevRailProject", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailProjectResponse), (status = 404, body = ErrorEnvelope)))]
fn get_devrail_project() {}
#[utoipa::path(patch, path = "/projects/{id}", operation_id = "updateDevRailProject", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = UpdateDevRailProjectRequest, responses((status = 200, body = DevRailProjectResponse), (status = 404, body = ErrorEnvelope)))]
fn update_devrail_project() {}
#[utoipa::path(post, path = "/projects/{id}/archive", operation_id = "archiveDevRailProject", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 204), (status = 404, body = ErrorEnvelope)))]
fn archive_devrail_project() {}
#[utoipa::path(get, path = "/projects/{id}/policy", operation_id = "getDevRailProjectPolicy", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailProjectPolicyResponse)))]
fn get_devrail_project_policy() {}
#[utoipa::path(patch, path = "/projects/{id}/policy", operation_id = "updateDevRailProjectPolicy", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = UpdateDevRailProjectPolicyRequest, responses((status = 200, body = DevRailProjectPolicyResponse), (status = 422, body = ErrorEnvelope)))]
fn update_devrail_project_policy() {}
#[utoipa::path(get, path = "/projects/{project_id}/members", operation_id = "listDevRailProjectMembers", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path)), responses((status = 200, body = DevRailProjectMemberPage)))]
fn list_devrail_project_members() {}
#[utoipa::path(post, path = "/projects/{project_id}/members", operation_id = "addDevRailProjectMember", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path)), request_body = AddDevRailProjectMemberRequest, responses((status = 201, body = DevRailProjectMemberResponse)))]
fn add_devrail_project_member() {}
#[utoipa::path(delete, path = "/projects/{project_id}/members/{user_id}", operation_id = "removeDevRailProjectMember", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("user_id" = i64, Path)), responses((status = 204)))]
fn remove_devrail_project_member() {}

#[utoipa::path(get, path = "/projects/{project_id}/repositories", operation_id = "listDevRailRepositories", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), DevRailListQuery), responses((status = 200, body = DevRailRepositoryPage)))]
fn list_devrail_repositories() {}
#[utoipa::path(post, path = "/projects/{project_id}/repositories", operation_id = "createDevRailRepository", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path)), request_body = CreateDevRailRepositoryRequest, responses((status = 201, body = DevRailRepositoryResponse)))]
fn create_devrail_repository() {}
#[utoipa::path(get, path = "/projects/{project_id}/repositories/{id}", operation_id = "getDevRailRepository", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), responses((status = 200, body = DevRailRepositoryResponse)))]
fn get_devrail_repository() {}
#[utoipa::path(patch, path = "/projects/{project_id}/repositories/{id}", operation_id = "updateDevRailRepository", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), request_body = UpdateDevRailRepositoryRequest, responses((status = 200, body = DevRailRepositoryResponse)))]
fn update_devrail_repository() {}

#[utoipa::path(get, path = "/projects/{project_id}/environments", operation_id = "listDevRailEnvironments", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), DevRailListQuery), responses((status = 200, body = DevRailEnvironmentPage)))]
fn list_devrail_environments() {}
#[utoipa::path(post, path = "/projects/{project_id}/environments", operation_id = "createDevRailEnvironment", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path)), request_body = CreateDevRailEnvironmentRequest, responses((status = 201, body = DevRailEnvironmentResponse)))]
fn create_devrail_environment() {}
#[utoipa::path(get, path = "/projects/{project_id}/environments/{id}", operation_id = "getDevRailEnvironment", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), responses((status = 200, body = DevRailEnvironmentResponse)))]
fn get_devrail_environment() {}
#[utoipa::path(patch, path = "/projects/{project_id}/environments/{id}", operation_id = "updateDevRailEnvironment", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), request_body = UpdateDevRailEnvironmentRequest, responses((status = 200, body = DevRailEnvironmentResponse)))]
fn update_devrail_environment() {}

#[utoipa::path(get, path = "/projects/{project_id}/tasks", operation_id = "listDevRailTasks", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), DevRailListQuery), responses((status = 200, body = DevRailTaskPage)))]
fn list_devrail_tasks() {}
#[utoipa::path(post, path = "/projects/{project_id}/tasks", operation_id = "createDevRailTask", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path)), request_body = CreateDevRailTaskRequest, responses((status = 201, body = DevRailTaskResponse)))]
fn create_devrail_task() {}
#[utoipa::path(get, path = "/projects/{project_id}/tasks/{id}", operation_id = "getDevRailTask", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), responses((status = 200, body = DevRailTaskResponse)))]
fn get_devrail_task() {}
#[utoipa::path(patch, path = "/projects/{project_id}/tasks/{id}", operation_id = "updateDevRailTask", tag = "devrail", security(("cookieAuth" = [])), params(("project_id" = i64, Path), ("id" = i64, Path)), request_body = UpdateDevRailTaskRequest, responses((status = 200, body = DevRailTaskResponse)))]
fn update_devrail_task() {}

#[utoipa::path(post, path = "/tasks/{task_id}/runs", operation_id = "createDevRailRun", tag = "devrail", security(("cookieAuth" = [])), params(("task_id" = i64, Path)), request_body = CreateDevRailRunRequest, responses((status = 202, body = DevRailRunResponse)))]
fn create_devrail_run() {}
#[utoipa::path(get, path = "/tasks/{task_id}/runs", operation_id = "listDevRailRuns", tag = "devrail", security(("cookieAuth" = [])), params(("task_id" = i64, Path), DevRailListQuery), responses((status = 200, body = DevRailRunPage)))]
fn list_devrail_runs() {}
#[utoipa::path(get, path = "/runs/{id}", operation_id = "getDevRailRun", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailRunResponse)))]
fn get_devrail_run() {}
#[utoipa::path(post, path = "/runs/{id}/interrupt", operation_id = "interruptDevRailRun", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailRunResponse)))]
fn interrupt_devrail_run() {}
#[utoipa::path(get, path = "/runs/{id}/events", operation_id = "listDevRailRunEvents", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailRunEventPage)))]
fn list_devrail_run_events() {}
#[utoipa::path(get, path = "/runs/{id}/changeset", operation_id = "getDevRailRunChangeset", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailChangesetResponse)))]
fn get_devrail_run_changeset() {}
#[utoipa::path(get, path = "/runs/{id}/quality-gates", operation_id = "getDevRailRunQualityGates", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailQualityGatePage)))]
fn get_devrail_run_quality_gates() {}
#[utoipa::path(get, path = "/runs/{id}/events/stream", operation_id = "streamDevRailRunEvents", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, description = "运行事件 SSE 流")))]
fn stream_devrail_run_events() {}
#[utoipa::path(post, path = "/runs/{id}/retry", operation_id = "retryDevRailRun", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = RetryDevRailRunRequest, responses((status = 202, body = DevRailRunResponse)))]
fn retry_devrail_run() {}
#[utoipa::path(get, path = "/approvals", operation_id = "listDevRailApprovals", tag = "devrail", security(("cookieAuth" = [])), params(DevRailListQuery), responses((status = 200, body = DevRailApprovalPage)))]
fn list_devrail_approvals() {}
#[utoipa::path(get, path = "/approvals/{id}", operation_id = "getDevRailApproval", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), responses((status = 200, body = DevRailApprovalResponse)))]
fn get_devrail_approval() {}
#[utoipa::path(post, path = "/approvals/{id}/approve", operation_id = "approveDevRailApproval", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = DevRailApprovalDecisionRequest, responses((status = 200, body = DevRailApprovalResponse)))]
fn approve_devrail_approval() {}
#[utoipa::path(post, path = "/approvals/{id}/reject", operation_id = "rejectDevRailApproval", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = DevRailApprovalDecisionRequest, responses((status = 200, body = DevRailApprovalResponse)))]
fn reject_devrail_approval() {}
#[utoipa::path(post, path = "/approvals/{id}/withdraw", operation_id = "withdrawDevRailApproval", tag = "devrail", security(("cookieAuth" = [])), params(("id" = i64, Path)), request_body = DevRailApprovalDecisionRequest, responses((status = 200, body = DevRailApprovalResponse)))]
fn withdraw_devrail_approval() {}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Arc Admin API",
        version = "2.4.0",
        description = "Arc Admin 母模板的认证、RBAC、用户与审计 API"
    ),
    paths(
        health_check,
        readiness_check,
        login,
        logout,
        current_user,
        change_current_user_password,
        issue_step_up_token,
        unlock_current_user_module,
        current_user_module_unlock_status,
        current_user_permissions,
        verify_mfa_totp,
        verify_mfa_recovery_code,
        start_mfa_passkey_authentication,
        finish_mfa_passkey_authentication,
        current_user_mfa_status,
        start_current_user_passkey_registration,
        finish_current_user_passkey_registration,
        revoke_current_user_passkey,
        regenerate_current_user_recovery_codes,
        list_users,
        create_user,
        get_user,
        update_user,
        delete_user,
        batch_delete_users,
        assign_user_roles,
        batch_assign_user_roles,
        list_roles,
        create_role,
        get_role,
        update_role,
        delete_role,
        get_role_permissions,
        update_role_permissions,
        list_departments,
        create_department,
        get_department,
        update_department,
        delete_department,
        list_permission_groups,
        dashboard_stats,
        list_audit_logs
        ,list_devrail_projects, create_devrail_project, get_devrail_project,
        update_devrail_project, archive_devrail_project, get_devrail_project_policy,
        update_devrail_project_policy, list_devrail_project_members,
        add_devrail_project_member, remove_devrail_project_member, list_devrail_repositories,
        create_devrail_repository, get_devrail_repository, update_devrail_repository,
        list_devrail_environments, create_devrail_environment, get_devrail_environment,
        update_devrail_environment, list_devrail_tasks, create_devrail_task,
        get_devrail_task, update_devrail_task, create_devrail_run, list_devrail_runs,
        get_devrail_run, interrupt_devrail_run, list_devrail_run_events, get_devrail_run_changeset, get_devrail_run_quality_gates
        ,stream_devrail_run_events, retry_devrail_run, list_devrail_approvals,
        get_devrail_approval, approve_devrail_approval, reject_devrail_approval, withdraw_devrail_approval
    ),
    components(schemas(
        ErrorEnvelope,
        UserStatusSchema,
        DepartmentStatusSchema,
        DataScopeSchema,
        RoleColorSchema,
        PermissionTypeSchema,
        UserSortBySchema,
        SortDirectionSchema,
        HealthResponse,
        ReadinessResponse,
        UserResponse,
        DepartmentResponse,
        RoleResponse,
        PermissionResponse,
        PermissionGroupResponse,
        LoginRequest,
        ChangePasswordRequest,
        StepUpRequest,
        StepUpResponse,
        ModuleUnlockScopeSchema,
        ModuleUnlockRequest,
        ModuleUnlockStatusResponse,
        LoginStatusSchema,
        MfaMethodSchema,
        LoginResponse,
        MfaCodeRequest,
        MfaPasskeyAuthenticationStartRequest,
        MfaPasskeyAuthenticationFinishRequest,
        MfaPasskeyRegistrationStartRequest,
        MfaPasskeyRegistrationFinishRequest,
        MfaFactorRevokeRequest,
        MfaWebauthnChallengeResponse,
        MfaPasskeyResponse,
        MfaStatusResponse,
        RecoveryCodesResponse,
        PageUser,
        PageAuditLog,
        CreateUserRequest,
        UpdateUserRequest,
        CreateDepartmentRequest,
        UpdateDepartmentRequest,
        AssignRolesRequest,
        BatchUserIdsRequest,
        BatchAssignRolesRequest,
        CreateRoleRequest,
        UpdateRoleRequest,
        RolePermissions,
        UpdateRolePermissionsRequest,
        PermissionCodes,
        DashboardStats,
        DevRailProjectResponse, DevRailProjectPolicyResponse, DevRailProjectPage, DevRailProjectMemberPage, DevRailProjectMemberResponse, AddDevRailProjectMemberRequest, DevRailRepositoryResponse,
        DevRailRepositoryPage, DevRailEnvironmentResponse, DevRailEnvironmentPage,
        DevRailTaskResponse, DevRailTaskPage, CreateDevRailProjectRequest,
        UpdateDevRailProjectRequest, UpdateDevRailProjectPolicyRequest, CreateDevRailRepositoryRequest,
        UpdateDevRailRepositoryRequest, CreateDevRailEnvironmentRequest,
        UpdateDevRailEnvironmentRequest, CreateDevRailTaskRequest,
        UpdateDevRailTaskRequest, CreateDevRailRunRequest, DevRailRunResponse,
        DevRailRunPage, DevRailRunEventPage, DevRailChangesetResponse, DevRailQualityGatePage, RetryDevRailRunRequest,
        DevRailApprovalResponse, DevRailApprovalPage, DevRailApprovalDecisionRequest
    )),
    servers((url = "/api/v1", description = "默认 API 根路径")),
    modifiers(&SecurityAddon),
    tags(
        (name = "system", description = "健康检查"),
        (name = "auth", description = "认证与当前会话"),
        (name = "users", description = "用户管理"),
        (name = "roles", description = "角色管理"),
        (name = "permissions", description = "权限目录"),
        (name = "dashboard", description = "仪表盘"),
        (name = "audit", description = "安全审计")
        ,(name = "devrail", description = "DevRail Harness 项目与任务")
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut OpenApiDocument) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                COOKIE_SECURITY,
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("__Host-arc_session"))),
            );
        }
    }
}

pub fn document() -> OpenApiDocument {
    ApiDoc::openapi()
}
