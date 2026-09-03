use sqlx::{AssertSqlSafe, PgConnection};

pub async fn upsert(
    c: &mut PgConnection,
    organization_id: i64,
    repository_id: i64,
    provider: &str,
    number: i64,
    url: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe("INSERT INTO devrail_pull_requests (organization_id,repository_id,provider,number,url,status,last_synced_at) VALUES ($1,$2,$3,$4,$5,$6,now()) ON CONFLICT (repository_id,provider,number) DO UPDATE SET url=EXCLUDED.url,status=EXCLUDED.status,last_synced_at=now(),updated_at=now()"))
        .bind(organization_id).bind(repository_id).bind(provider).bind(number).bind(url).bind(status).execute(&mut *c).await.map(|_| ())
}
pub async fn update_webhook(
    c: &mut PgConnection,
    organization_id: i64,
    provider: &str,
    repository_id: i64,
    number: i64,
    url: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(AssertSqlSafe("UPDATE devrail_pull_requests p SET url=$5,status=$6,last_synced_at=now(),updated_at=now() WHERE p.organization_id=$1 AND p.provider=$2 AND p.repository_id=$3 AND p.number=$4"))
        .bind(organization_id).bind(provider).bind(repository_id).bind(number).bind(url).bind(status).execute(&mut *c).await?;
    Ok(result.rows_affected() > 0)
}
pub async fn claim_event(
    c: &mut PgConnection,
    provider: &str,
    event_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("INSERT INTO devrail_webhook_events (provider,event_id) VALUES ($1,$2) ON CONFLICT DO NOTHING").bind(provider).bind(event_id).execute(&mut *c).await?;
    Ok(result.rows_affected() == 1)
}
pub async fn repository_owner(
    c: &mut PgConnection,
    repository_id: i64,
) -> Result<Option<(i64, Option<i64>, i64)>, sqlx::Error> {
    sqlx::query_as("SELECT organization_id,department_id,owner_user_id FROM devrail_repositories WHERE id=$1 AND archived_at IS NULL").bind(repository_id).fetch_optional(&mut *c).await
}
