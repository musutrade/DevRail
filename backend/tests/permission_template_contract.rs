use arc_admin_backend::auth::PermissionRequirement;
use arc_admin_backend::permissions::{AuditLogRead, UserRead};
use std::collections::BTreeSet;

const SQL_TEMPLATE: &str =
    include_str!("../../codex-audit-pipeline/.codex/templates/business_permissions.sql.tmpl");
const RUST_TEMPLATE: &str =
    include_str!("../../codex-audit-pipeline/.codex/templates/rust_permissions.rs.tmpl");
const ANGULAR_TEMPLATE: &str =
    include_str!("../../codex-audit-pipeline/.codex/templates/angular_permissions.ts.tmpl");
const HANDLER_TEMPLATE: &str =
    include_str!("../../codex-audit-pipeline/.codex/templates/rust_handler.rs.tmpl");

const READ_CODE: &str = "{{PERMISSION_PREFIX}}:{{RESOURCE_NAME}}:read";
const WRITE_CODE: &str = "{{PERMISSION_PREFIX}}:{{RESOURCE_NAME}}:write";

#[test]
fn platform_permission_markers_keep_their_codes() {
    assert_eq!(UserRead::CODE, "user:directory:read");
    assert_eq!(AuditLogRead::CODE, "audit:logs:read");
}

#[test]
fn business_templates_share_the_same_permission_codes() {
    for template in [SQL_TEMPLATE, RUST_TEMPLATE, ANGULAR_TEMPLATE] {
        assert!(template.contains(READ_CODE));
        assert!(template.contains(WRITE_CODE));
    }
}

#[test]
fn migration_template_grants_only_the_super_admin_by_default() {
    assert!(SQL_TEMPLATE.contains("WHERE r.code = 'super_admin'"));
    assert!(SQL_TEMPLATE.contains("ON CONFLICT DO NOTHING"));
    assert!(!SQL_TEMPLATE.contains("WHERE r.code IN"));
}

#[test]
fn handler_template_requires_typed_read_and_write_permissions() {
    assert_eq!(
        HANDLER_TEMPLATE
            .matches("RequirePermission<{{READ_PERMISSION_TYPE}}>")
            .count(),
        2
    );
    assert_eq!(
        HANDLER_TEMPLATE
            .matches("RequirePermission<{{WRITE_PERMISSION_TYPE}}>")
            .count(),
        2
    );
    assert!(!HANDLER_TEMPLATE.contains("AuthUser"));
}

#[test]
fn template_placeholder_contract_is_explicit() {
    assert_eq!(
        placeholders(SQL_TEMPLATE),
        expected(&[
            "GROUP_ICON",
            "GROUP_NAME",
            "PERMISSION_PREFIX",
            "RESOURCE_LABEL",
            "RESOURCE_NAME",
        ])
    );
    assert_eq!(
        placeholders(RUST_TEMPLATE),
        expected(&[
            "PERMISSION_MODULE",
            "PERMISSION_PREFIX",
            "READ_PERMISSION_TYPE",
            "RESOURCE_NAME",
            "WRITE_PERMISSION_TYPE",
        ])
    );
    assert_eq!(
        placeholders(ANGULAR_TEMPLATE),
        expected(&[
            "PERMISSION_CONST",
            "PERMISSION_PREFIX",
            "RESOURCE_NAME",
            "ROUTE_ACCESS_CONST",
        ])
    );
}

fn placeholders(template: &str) -> BTreeSet<String> {
    template
        .split("{{")
        .skip(1)
        .filter_map(|part| part.split_once("}}").map(|(name, _)| name.to_string()))
        .collect()
}

fn expected(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}
