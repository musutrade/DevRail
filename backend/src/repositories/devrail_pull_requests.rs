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
