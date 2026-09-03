use arc_admin_backend::db::test_schema_pool;
use arc_admin_backend::repositories::mfa;
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

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

#[tokio::test]
async fn totp_challenge_counters_are_atomic_and_purpose_scoped() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    let fixture = test_schema_pool().await.expect("schema fixture");
    let pool = fixture.pool();
    let organization_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO organizations (code,name) VALUES ($1,'MFA 重放测试组织') RETURNING id",
    )
    .bind(format!("mfa-replay-{}", Uuid::new_v4().simple()))
    .fetch_one(pool)
    .await
    .expect("create organization");
    let department_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO departments (organization_id,code,name)
         VALUES ($1,'root','根部门') RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(pool)
    .await
    .expect("create department");
    let user_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (username,password_hash,display_name,organization_id,department_id)
         VALUES ($1,'test','MFA 测试用户',$2,$3) RETURNING id",
    )
    .bind(format!("mfa-replay-{}", Uuid::new_v4().simple()))
    .bind(organization_id)
    .bind(department_id)
    .fetch_one(pool)
    .await
    .expect("create user");
    let expires_at = Utc::now() + Duration::minutes(5);

    let mut first = pool.begin().await.expect("first challenge transaction");
    mfa::ensure_settings(&mut first, user_id, Uuid::new_v4())
        .await
        .expect("ensure settings");
    mfa::create_challenge(
        &mut first,
        &"a".repeat(64),
        user_id,
        "login",
        false,
        &json!({}),
        expires_at,
    )
    .await
    .expect("create first login challenge");
    let first_id =
        sqlx::query_scalar::<_, i64>("SELECT id FROM auth_mfa_challenges WHERE token_hash=$1")
            .bind("a".repeat(64))
            .fetch_one(&mut *first)
            .await
            .expect("first challenge id");
    assert!(
        mfa::consume_totp_challenge(&mut first, first_id, "login", 42)
            .await
            .expect("consume first login counter")
    );
    first.commit().await.expect("commit first challenge");

    let mut replay = pool.begin().await.expect("replay transaction");
    mfa::create_challenge(
        &mut replay,
        &"b".repeat(64),
        user_id,
        "login",
        false,
        &json!({}),
        expires_at,
    )
    .await
    .expect("create replay login challenge");
    let replay_id =
        sqlx::query_scalar::<_, i64>("SELECT id FROM auth_mfa_challenges WHERE token_hash=$1")
            .bind("b".repeat(64))
            .fetch_one(&mut *replay)
            .await
            .expect("replay challenge id");
    let replay_error = mfa::consume_totp_challenge(&mut replay, replay_id, "login", 42)
        .await
        .expect_err("same-purpose counter replay must fail");
    assert!(replay_error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation()));
    replay.rollback().await.expect("rollback replay");

    let mut enrollment = pool.begin().await.expect("enrollment transaction");
    mfa::create_challenge(
        &mut enrollment,
        &"c".repeat(64),
        user_id,
        "totp_enrollment",
        false,
        &json!({}),
        expires_at,
    )
    .await
    .expect("create enrollment challenge");
    let enrollment_id =
        sqlx::query_scalar::<_, i64>("SELECT id FROM auth_mfa_challenges WHERE token_hash=$1")
            .bind("c".repeat(64))
            .fetch_one(&mut *enrollment)
            .await
            .expect("enrollment challenge id");
    assert!(
        mfa::consume_totp_challenge(&mut enrollment, enrollment_id, "totp_enrollment", 42,)
            .await
            .expect("same counter in another purpose")
    );
    enrollment.commit().await.expect("commit enrollment");

    let mut concurrent_setup = pool.begin().await.expect("concurrent challenge setup");
    mfa::create_challenge(
        &mut concurrent_setup,
        &"d".repeat(64),
        user_id,
        "login",
        false,
        &json!({}),
        expires_at,
    )
    .await
    .expect("create concurrent login challenge");
    let concurrent_id =
        sqlx::query_scalar::<_, i64>("SELECT id FROM auth_mfa_challenges WHERE token_hash=$1")
            .bind("d".repeat(64))
            .fetch_one(&mut *concurrent_setup)
            .await
            .expect("concurrent challenge id");
    concurrent_setup
        .commit()
        .await
        .expect("commit concurrent setup");

    let first_pool = pool.clone();
    let second_pool = pool.clone();
    let first = tokio::spawn(async move {
        let mut transaction = first_pool
            .begin()
            .await
            .expect("first concurrent transaction");
        let result = mfa::consume_totp_challenge(&mut transaction, concurrent_id, "login", 43)
            .await
            .expect("first concurrent consume");
        transaction
            .commit()
            .await
            .expect("commit first concurrent consume");
        result
    });
    let second = tokio::spawn(async move {
        let mut transaction = second_pool
            .begin()
            .await
            .expect("second concurrent transaction");
        let result = mfa::consume_totp_challenge(&mut transaction, concurrent_id, "login", 43)
            .await
            .expect("second concurrent consume");
        transaction
            .commit()
            .await
            .expect("commit second concurrent consume");
        result
    });
    let (first_result, second_result) = tokio::join!(first, second);
    let first_result = first_result.expect("join first concurrent consume");
    let second_result = second_result.expect("join second concurrent consume");
    assert_ne!(first_result, second_result);

    let reauth_counter = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT last_reauth_totp_counter FROM user_mfa_settings WHERE user_id=$1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("read reauthentication counter");
    assert_eq!(reauth_counter, None);

    fixture.cleanup().await.expect("cleanup schema fixture");
}
