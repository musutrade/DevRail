//! 部门 Repository：层级查询与写操作的唯一数据访问层。

use crate::access::ActorContext;
use crate::models::DepartmentRow;
use sqlx::{PgConnection, PgPool};

pub(crate) struct NewDepartment {
    pub(crate) parent_id: i64,
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) status: String,
}

pub(crate) struct DepartmentUpdate {
    pub(crate) parent_id: Option<i64>,
    pub(crate) code: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) status: Option<String>,
}

pub async fn list(pool: &PgPool, actor: &ActorContext) -> Result<Vec<DepartmentRow>, sqlx::Error> {
    sqlx::query_as::<_, DepartmentRow>(
        "WITH RECURSIVE visible_departments AS (
             SELECT d.id, d.organization_id, d.parent_id, d.code, d.name, d.status,
                    d.created_at, d.updated_at, 0::integer AS depth,
                    ARRAY[d.id]::bigint[] AS tree_path
             FROM departments d
             WHERE d.organization_id = $2
               AND (
                   $1 IN ('all', 'organization') AND d.parent_id IS NULL
                   OR $1 IN ('department_and_children', 'department', 'self') AND d.id = $3
               )
             UNION ALL
             SELECT child.id, child.organization_id, child.parent_id, child.code, child.name,
                    child.status, child.created_at, child.updated_at, parent.depth + 1,
                    parent.tree_path || child.id
             FROM departments child
             JOIN visible_departments parent ON child.parent_id = parent.id
             WHERE child.organization_id = $2
               AND $1 IN ('all', 'organization', 'department_and_children')
         )
         SELECT d.id, d.organization_id, d.parent_id, d.code, d.name, d.status, d.depth,
                (SELECT count(*) FROM users u
                 WHERE u.department_id = d.id AND u.deleted_at IS NULL) AS member_count,
                (SELECT count(*) FROM departments child
                 WHERE child.parent_id = d.id) AS child_count,
                d.created_at, d.updated_at
         FROM visible_departments d
         ORDER BY d.tree_path",
    )
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.department_id)
    .fetch_all(pool)
    .await
}

pub async fn find_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DepartmentRow>, sqlx::Error> {
    sqlx::query_as::<_, DepartmentRow>(
        "WITH RECURSIVE visible_departments AS (
             SELECT d.id, d.organization_id, d.parent_id, d.code, d.name, d.status,
                    d.created_at, d.updated_at, 0::integer AS depth
             FROM departments d
             WHERE d.organization_id = $2
               AND (
                   $1 IN ('all', 'organization') AND d.parent_id IS NULL
                   OR $1 IN ('department_and_children', 'department', 'self') AND d.id = $3
               )
             UNION ALL
             SELECT child.id, child.organization_id, child.parent_id, child.code, child.name,
                    child.status, child.created_at, child.updated_at, parent.depth + 1
             FROM departments child
             JOIN visible_departments parent ON child.parent_id = parent.id
             WHERE child.organization_id = $2
               AND $1 IN ('all', 'organization', 'department_and_children')
         )
         SELECT d.id, d.organization_id, d.parent_id, d.code, d.name, d.status, d.depth,
                (SELECT count(*) FROM users u
                 WHERE u.department_id = d.id AND u.deleted_at IS NULL) AS member_count,
                (SELECT count(*) FROM departments child
                 WHERE child.parent_id = d.id) AS child_count,
                d.created_at, d.updated_at
         FROM visible_departments d
         WHERE d.id = $4",
    )
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.department_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn code_exists(
    pool: &PgPool,
    organization_id: i64,
    code: &str,
    exclude_id: Option<i64>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM departments
             WHERE organization_id = $1 AND code = $2
               AND ($3::bigint IS NULL OR id <> $3)
         )",
    )
    .bind(organization_id)
    .bind(code)
    .bind(exclude_id)
    .fetch_one(pool)
    .await
}

pub async fn parent_would_create_cycle(
    pool: &PgPool,
    organization_id: i64,
    department_id: i64,
    parent_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE descendants AS (
             SELECT id FROM departments
             WHERE id = $2 AND organization_id = $1
             UNION ALL
             SELECT child.id FROM departments child
             JOIN descendants parent ON child.parent_id = parent.id
             WHERE child.organization_id = $1
         )
         SELECT EXISTS(SELECT 1 FROM descendants WHERE id = $3)",
    )
    .bind(organization_id)
    .bind(department_id)
    .bind(parent_id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn create(
    connection: &mut PgConnection,
    organization_id: i64,
    department: &NewDepartment,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO departments (organization_id, parent_id, code, name, status)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(organization_id)
    .bind(department.parent_id)
    .bind(&department.code)
    .bind(&department.name)
    .bind(&department.status)
    .fetch_one(connection)
    .await
}

pub(crate) async fn update(
    connection: &mut PgConnection,
    organization_id: i64,
    id: i64,
    department: &DepartmentUpdate,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE departments
         SET parent_id = COALESCE($3, parent_id),
             code = COALESCE($4, code),
             name = COALESCE($5, name),
             status = COALESCE($6, status),
             updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(organization_id)
    .bind(department.parent_id)
    .bind(&department.code)
    .bind(&department.name)
    .bind(&department.status)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub(crate) async fn delete_if_empty(
    connection: &mut PgConnection,
    organization_id: i64,
    id: i64,
) -> Result<bool, sqlx::Error> {
    // Soft-deleted users are historical records and should not keep a department
    // alive through the foreign key after the department has no active members.
    sqlx::query(
        "UPDATE users
         SET department_id = NULL, updated_at = now()
         WHERE department_id = $1 AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .execute(&mut *connection)
    .await?;

    let result = sqlx::query(
        "DELETE FROM departments d
         WHERE d.id = $1 AND d.organization_id = $2 AND d.parent_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM departments child WHERE child.parent_id = d.id)
           AND NOT EXISTS (
               SELECT 1 FROM users u
               WHERE u.department_id = d.id AND u.deleted_at IS NULL
           )",
    )
    .bind(id)
    .bind(organization_id)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() > 0)
}
