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
    provider: &str,
    repository_id: i64,
    number: i64,
    url: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(AssertSqlSafe("UPDATE devrail_pull_requests p SET url=$4,status=$5,last_synced_at=now(),updated_at=now() WHERE p.provider=$1 AND p.repository_id=$2 AND p.number=$3"))
        .bind(provider).bind(repository_id).bind(number).bind(url).bind(status).execute(&mut *c).await?;
    Ok(result.rows_affected() > 0)
}
