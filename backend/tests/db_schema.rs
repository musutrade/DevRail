use arc_admin_backend::db::test_schema_pool;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_schema_fixtures_are_isolated_and_cleanup_connections() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    let (first, second) = tokio::join!(test_schema_pool(), test_schema_pool());
    let first = first.expect("first schema fixture");
    let second = match second {
        Ok(fixture) => fixture,
        Err(error) => {
            first.cleanup().await.expect("cleanup first schema");
            panic!("second schema fixture: {error:#}");
        }
    };
    assert_ne!(first.schema(), second.schema());

    let first_value = "schema-fixture-first";
    let second_value = "schema-fixture-second";
    sqlx::query("CREATE TABLE fixture_values (value TEXT NOT NULL)")
        .execute(first.pool())
        .await
        .expect("create first fixture table");
    sqlx::query("CREATE TABLE fixture_values (value TEXT NOT NULL)")
        .execute(second.pool())
        .await
        .expect("create second fixture table");
    sqlx::query("INSERT INTO fixture_values (value) VALUES ($1)")
        .bind(first_value)
        .execute(first.pool())
        .await
        .expect("insert first fixture value");
    sqlx::query("INSERT INTO fixture_values (value) VALUES ($1)")
        .bind(second_value)
        .execute(second.pool())
        .await
        .expect("insert second fixture value");

    let first_read = sqlx::query_scalar::<_, String>("SELECT value FROM fixture_values")
        .fetch_one(first.pool())
        .await
        .expect("read first fixture value");
    let second_read = sqlx::query_scalar::<_, String>("SELECT value FROM fixture_values")
        .fetch_one(second.pool())
        .await
        .expect("read second fixture value");
    assert_eq!(first_read, first_value);
    assert_eq!(second_read, second_value);

    let first_schema = sqlx::query_scalar::<_, String>("SELECT current_schema()")
        .fetch_one(first.pool())
        .await
        .expect("read first search path");
    let second_schema = sqlx::query_scalar::<_, String>("SELECT current_schema()")
        .fetch_one(second.pool())
        .await
        .expect("read second search path");
    assert_eq!(first_schema, first.schema());
    assert_eq!(second_schema, second.schema());

    first.cleanup().await.expect("cleanup first schema");
    second.cleanup().await.expect("cleanup second schema");
}

#[tokio::test]
async fn schema_fixture_surfaces_cleanup_failure_after_external_drop() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    let fixture = test_schema_pool().await.expect("schema fixture");
    let drop_statement = format!(
        "DROP SCHEMA \"{}\" CASCADE",
        fixture.schema().replace('"', "\"\"")
    );
    sqlx::query(sqlx::AssertSqlSafe(drop_statement))
        .execute(fixture.pool())
        .await
        .expect("external schema drop");

    assert!(fixture.cleanup().await.is_err());
}
