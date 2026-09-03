//! 角色 Repository：唯一的角色数据访问层（SQL 写操作只允许出现在这里）

use crate::models::{RoleRow, RoleWithPermissionsRow};
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};

const ROLE_SELECT: &str = "SELECT r.id, r.code, r.name, r.category, r.icon, r.color, \
    r.description, r.data_scope, r.is_active, \
    (SELECT count(*) FROM user_roles ur \
     JOIN users u ON u.id = ur.user_id AND u.deleted_at IS NULL \
     WHERE ur.role_id = r.id) AS members \
    FROM roles r";

pub(crate) struct NewRole {
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) icon: Option<String>,
    pub(crate) color: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) data_scope: String,
}

pub(crate) enum NullableTextUpdate {
    Unchanged,
    Set(Option<String>),
}

impl NullableTextUpdate {
    fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    fn value(&self) -> Option<&str> {
        match self {
            Self::Unchanged | Self::Set(None) => None,
            Self::Set(Some(value)) => Some(value),
        }
    }
}

pub(crate) struct RoleUpdate {
    pub(crate) name: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) icon: NullableTextUpdate,
    pub(crate) color: Option<String>,
    pub(crate) description: NullableTextUpdate,
    pub(crate) data_scope: Option<String>,
    pub(crate) is_active: Option<bool>,
}

pub async fn list_all(pool: &PgPool) -> Result<Vec<RoleWithPermissionsRow>, sqlx::Error> {
    sqlx::query_as::<_, RoleWithPermissionsRow>(
        "SELECT r.id, r.code, r.name, r.category, r.icon, r.color, r.description,
                r.data_scope, r.is_active,
                (SELECT count(*) FROM user_roles ur
                 JOIN users u ON u.id = ur.user_id AND u.deleted_at IS NULL
                 WHERE ur.role_id = r.id) AS members,
                COALESCE(
                    (
                        SELECT array_agg(DISTINCT p.group_id ORDER BY p.group_id)
                        FROM role_permissions rp
                        JOIN permissions p ON p.id = rp.permission_id
                        WHERE rp.role_id = r.id
                    ),
                    ARRAY[]::bigint[]
                ) AS permission_group_ids
         FROM roles r
         ORDER BY r.id",
    )
    .fetch_all(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<RoleRow>, sqlx::Error> {
    sqlx::query_as::<_, RoleRow>(AssertSqlSafe(format!("{ROLE_SELECT} WHERE r.id = $1")))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn create(
    connection: &mut PgConnection,
    role: &NewRole,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO roles (code, name, category, icon, color, description, data_scope)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(&role.code)
    .bind(&role.name)
    .bind(&role.category)
    .bind(&role.icon)
    .bind(&role.color)
    .bind(&role.description)
    .bind(&role.data_scope)
    .fetch_one(&mut *connection)
    .await?;
    row.try_get::<i64, _>(0)
}

pub async fn id_by_code(pool: &PgPool, code: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM roles WHERE code = $1 AND is_active = TRUE")
        .bind(code)
        .fetch_optional(pool)
        .await
}

pub async fn active_ids_by_ids(pool: &PgPool, role_ids: &[i64]) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM roles
         WHERE id = ANY($1::bigint[]) AND is_active = TRUE
         ORDER BY id",
    )
    .bind(role_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn update(
    connection: &mut PgConnection,
    id: i64,
    role: &RoleUpdate,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE roles
         SET name = COALESCE($2, name),
             category = COALESCE($3, category),
             icon = CASE WHEN $4 THEN $5 ELSE icon END,
             color = COALESCE($6, color),
             description = CASE WHEN $7 THEN $8 ELSE description END,
             data_scope = COALESCE($9, data_scope),
             is_active = COALESCE($10, is_active),
             updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&role.name)
    .bind(&role.category)
    .bind(role.icon.is_set())
    .bind(role.icon.value())
    .bind(&role.color)
    .bind(role.description.is_set())
    .bind(role.description.value())
    .bind(&role.data_scope)
    .bind(role.is_active)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_if_unassigned(
    connection: &mut PgConnection,
    id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM roles r
         WHERE r.id = $1
           AND NOT EXISTS (
               SELECT 1 FROM user_roles ur
               JOIN users u ON u.id = ur.user_id AND u.deleted_at IS NULL
               WHERE ur.role_id = r.id
           )",
    )
    .bind(id)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn permission_ids_by_role(pool: &PgPool, role_id: i64) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT permission_id FROM role_permissions WHERE role_id = $1 ORDER BY permission_id",
    )
    .bind(role_id)
    .fetch_all(pool)
    .await
}

pub async fn permission_ids_by_role_in_connection(
    connection: &mut PgConnection,
    role_id: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT permission_id FROM role_permissions WHERE role_id = $1 ORDER BY permission_id",
    )
    .bind(role_id)
    .fetch_all(connection)
    .await
}

pub async fn permission_group_ids_by_role(
    pool: &PgPool,
    role_id: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT pg.id FROM permission_groups pg
         JOIN permissions p ON p.group_id = pg.id
         JOIN role_permissions rp ON rp.permission_id = p.id
         WHERE rp.role_id = $1
         ORDER BY pg.id",
    )
    .bind(role_id)
    .fetch_all(pool)
    .await
}

pub async fn assign_permissions(
    connection: &mut PgConnection,
    role_id: i64,
    permission_ids: &[i64],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM role_permissions WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *connection)
        .await?;
    for permission_id in permission_ids {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_id) VALUES ($1, $2)")
            .bind(role_id)
            .bind(permission_id)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}
