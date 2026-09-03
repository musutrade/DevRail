use arc_admin_backend::auth::{self, AuthSessionConfig};
use arc_admin_backend::mfa::MfaConfig;
use arc_admin_backend::models::{NullablePatch, UpdateUserRequest};
use arc_admin_backend::workers::harness_supervisor::HarnessSupervisor;
use arc_admin_backend::{build_router, db, repositories, services, AppState};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;
use totp_rs::{Algorithm, Builder, Secret, Totp};
use tower::ServiceExt;

const MFA_TEST_STEP_SECS: u64 = 3;

#[derive(Debug)]
struct TestSession {
    username: String,
    password: String,
    cookie_header: String,
    csrf_token: String,
    session_token: String,
    session_set_cookie: String,
    csrf_set_cookie: String,
    totp_secret: Option<String>,
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    session: Option<&TestSession>,
    body: Option<Value>,
    include_csrf: bool,
    forwarded_for: Option<&str>,
) -> Response<Body> {
    let mut builder = Request::builder().method(method.clone()).uri(uri);
    if let Some(forwarded_for) = forwarded_for {
        builder = builder.header("x-forwarded-for", forwarded_for);
    }
    if let Some(session) = session {
        builder = builder.header(COOKIE, &session.cookie_header);
        if include_csrf && !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
            builder = builder.header("x-csrf-token", &session.csrf_token);
        }
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("router response")
}

async fn response_json(response: Response<Body>) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response JSON")
    };
    (status, value)
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    session: Option<&TestSession>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    if let (Some(session), Some(scope)) = (session, sensitive_scope(&method, uri, body.as_ref())) {
        let step_up_token = issue_step_up(app, session, scope).await;
        return response_json(
            request_with_step_up(app, method, uri, session, body, &step_up_token).await,
        )
        .await;
    }
    if let (Some(session), Some(module)) = (session, routine_write_module(&method, uri)) {
        issue_module_unlock(app, session, module).await;
    }
    response_json(request(app, method, uri, session, body, true, None).await).await
}

fn routine_write_module(method: &Method, uri: &str) -> Option<&'static str> {
    let path = uri.split('?').next().unwrap_or(uri);
    match (method, path) {
        (&Method::POST, "/api/v1/users") => Some("users"),
        (&Method::PUT, path) if path.starts_with("/api/v1/users/") && !path.ends_with("/roles") => {
            Some("users")
        }
        (&Method::POST, "/api/v1/roles") => Some("roles"),
        (&Method::PUT, path)
            if path.starts_with("/api/v1/roles/") && !path.ends_with("/permissions") =>
        {
            Some("roles")
        }
        _ => None,
    }
}

fn sensitive_scope(method: &Method, uri: &str, body: Option<&Value>) -> Option<&'static str> {
    let path = uri.split('?').next().unwrap_or(uri);
    let object = body.and_then(Value::as_object);
    match (method, path) {
        (&Method::PUT, "/api/v1/auth/me/password") => Some("auth.password.change"),
        (&Method::POST, "/api/v1/users/batch-delete") => Some("users.delete"),
        (&Method::PUT, "/api/v1/users/batch-roles") => Some("users.roles.write"),
        (&Method::DELETE, path) if path.starts_with("/api/v1/users/") => {
            if path.ends_with("/roles") {
                None
            } else {
                Some("users.delete")
            }
        }
        (&Method::PUT, path) if path.starts_with("/api/v1/users/") && path.ends_with("/roles") => {
            Some("users.roles.write")
        }
        (&Method::POST, "/api/v1/users") => {
            let has_roles = object
                .and_then(|value| value.get("roleIds"))
                .and_then(Value::as_array)
                .is_some_and(|ids| !ids.is_empty());
            let has_inactive_status = object
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .is_some_and(|status| status != "active");
            let has_department = object.is_some_and(|value| value.contains_key("departmentId"));
            (has_roles || has_inactive_status || has_department).then_some("users.sensitive")
        }
        (&Method::PUT, path) if path.starts_with("/api/v1/users/") => object
            .is_some_and(|value| {
                value.contains_key("password")
                    || value.contains_key("status")
                    || value.contains_key("departmentId")
            })
            .then_some("users.sensitive"),
        (&Method::DELETE, path) if path.starts_with("/api/v1/roles/") => {
            (!path.ends_with("/permissions")).then_some("roles.delete")
        }
        (&Method::PUT, path)
            if path.starts_with("/api/v1/roles/") && path.ends_with("/permissions") =>
        {
            Some("roles.permissions.write")
        }
        (&Method::POST, "/api/v1/roles") => {
            let has_permissions = object
                .and_then(|value| value.get("permissionIds"))
                .and_then(Value::as_array)
                .is_some_and(|ids| !ids.is_empty());
            has_permissions.then_some("roles.permissions.write")
        }
        (&Method::PUT, path) if path.starts_with("/api/v1/roles/") => object
            .is_some_and(|value| value.contains_key("dataScope") || value.contains_key("isActive"))
            .then_some("roles.sensitive"),
        (&Method::POST, "/api/v1/departments") => Some("departments.write"),
        (&Method::PUT, path) if path.starts_with("/api/v1/departments/") => {
            Some("departments.write")
        }
        (&Method::DELETE, path) if path.starts_with("/api/v1/departments/") => {
            Some("departments.delete")
        }
        _ => None,
    }
}

async fn issue_step_up(app: &Router, session: &TestSession, scope: &str) -> String {
    issue_step_up_with_code(app, session, scope).await.0
}

async fn issue_step_up_with_code(
    app: &Router,
    session: &TestSession,
    scope: &str,
) -> (String, Option<String>) {
    for _ in 0..50 {
        let totp_code = current_totp_code(session);
        let (status, body) = response_json(
            request(
                app,
                Method::POST,
                "/api/v1/auth/me/step-up",
                Some(session),
                Some(json!({
                    "currentPassword": session.password,
                    "totpCode": totp_code,
                    "scope": scope,
                })),
                true,
                None,
            )
            .await,
        )
        .await;
        if status == StatusCode::OK {
            return (
                body["token"].as_str().expect("step-up token").to_string(),
                totp_code,
            );
        }
        if body["error"]["message"] != "验证码已使用或已过期，请输入最新验证码" {
            panic!("step-up failed with {status}: {body}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("TOTP test counter did not advance")
}

async fn issue_module_unlock(app: &Router, session: &TestSession, module: &str) {
    let (status, body) = response_json(
        request(
            app,
            Method::GET,
            &format!("/api/v1/auth/me/module-unlocks/{module}"),
            Some(session),
            None,
            false,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "module status failed: {body}");
    if body["unlocked"] == true {
        return;
    }

    for _ in 0..50 {
        let (status, body) = response_json(
            request(
                app,
                Method::POST,
                "/api/v1/auth/me/module-unlocks",
                Some(session),
                Some(json!({
                    "module": module,
                    "currentPassword": session.password,
                    "totpCode": current_totp_code(session),
                })),
                true,
                None,
            )
            .await,
        )
        .await;
        if status == StatusCode::OK {
            assert_eq!(body["module"], module);
            assert_eq!(body["unlocked"], true);
            return;
        }
        if body["error"]["message"] != "验证码已使用或已过期，请输入最新验证码" {
            panic!("module unlock failed with {status}: {body}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("TOTP test counter did not advance")
}

fn current_totp_code(session: &TestSession) -> Option<String> {
    session.totp_secret.as_deref().map(|secret| {
        test_totp(&session.username, secret)
            .generate_current()
            .to_string()
    })
}

fn test_totp(account_name: &str, secret: &str) -> Totp {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(MFA_TEST_STEP_SECS)
        .with_secret(Secret::try_from_base32(secret).expect("TOTP secret bytes"))
        .with_account_name(account_name)
        .build()
        .expect("test TOTP")
}

async fn request_with_step_up(
    app: &Router,
    method: Method,
    uri: &str,
    session: &TestSession,
    body: Option<Value>,
    step_up_token: &str,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header(COOKIE, &session.cookie_header)
        .header("x-csrf-token", &session.csrf_token)
        .header("x-step-up-token", step_up_token);
    let body = match body {
        Some(value) => {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("router response")
}

async fn send_from_ip(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    forwarded_for: &str,
) -> (StatusCode, Value) {
    response_json(request(app, method, uri, None, body, true, Some(forwarded_for)).await).await
}

async fn login(
    app: &Router,
    username: &str,
    password: &str,
    remember: bool,
) -> (StatusCode, Value, Option<TestSession>) {
    login_with_totp_secret(app, username, password, remember, None).await
}

async fn login_with_totp_secret(
    app: &Router,
    username: &str,
    password: &str,
    remember: bool,
    existing_secret: Option<&str>,
) -> (StatusCode, Value, Option<TestSession>) {
    let response = request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": username,
            "password": password,
            "remember": remember,
        })),
        false,
        None,
    )
    .await;
    let (status, body, session) = decode_login_response(response, username, password, None).await;
    if status != StatusCode::OK || session.is_some() || body["status"] == "authenticated" {
        return (status, body, session);
    }

    let secret = if body["status"] == "mfaEnrollmentRequired" {
        let secret = body["totpSecret"]
            .as_str()
            .expect("TOTP enrollment secret")
            .to_string();
        secret
    } else {
        existing_secret.map(str::to_string).unwrap_or_else(|| {
            panic!("login requires an existing session TOTP secret for {username}")
        })
    };
    let code = test_totp(username, &secret).generate_current().to_string();
    let response = request(
        app,
        Method::POST,
        "/api/v1/auth/mfa/totp/verify",
        None,
        Some(json!({
            "challengeToken": body["challengeToken"],
            "code": code,
        })),
        false,
        None,
    )
    .await;
    decode_login_response(response, username, password, Some(secret)).await
}

async fn decode_login_response(
    response: Response<Body>,
    username: &str,
    password: &str,
    totp_secret: Option<String>,
) -> (StatusCode, Value, Option<TestSession>) {
    let cookies = response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let session_set_cookie = cookies
        .iter()
        .find(|cookie| cookie.starts_with("arc_session="))
        .cloned();
    let csrf_set_cookie = cookies
        .iter()
        .find(|cookie| cookie.starts_with("arc_csrf="))
        .cloned();
    let test_session =
        session_set_cookie
            .zip(csrf_set_cookie)
            .map(|(session_set_cookie, csrf_set_cookie)| {
                let session_cookie = session_set_cookie
                    .split(';')
                    .next()
                    .expect("session cookie pair");
                let csrf_cookie = csrf_set_cookie.split(';').next().expect("CSRF cookie pair");
                let session_token = session_cookie
                    .split_once('=')
                    .expect("session cookie value")
                    .1
                    .to_string();
                let csrf_token = csrf_cookie
                    .split_once('=')
                    .expect("CSRF cookie value")
                    .1
                    .to_string();
                TestSession {
                    username: username.to_string(),
                    password: password.to_string(),
                    cookie_header: format!("{session_cookie}; {csrf_cookie}"),
                    csrf_token,
                    session_token,
                    session_set_cookie,
                    csrf_set_cookie,
                    totp_secret,
                }
            });
    let (status, body) = response_json(response).await;
    (status, body, test_session)
}

struct ApiTestFixture {
    schema: db::TestSchemaPool,
    pool: sqlx::PgPool,
    app: Router,
}

async fn api_test_fixture() -> ApiTestFixture {
    db::report_test_threads();
    let schema = db::test_schema_pool()
        .await
        .expect("schema-isolated test pool");
    let pool = schema.pool().clone();
    let app = build_router(AppState {
        pool: pool.clone(),
        auth: Arc::new(AuthSessionConfig {
            session_ttl_secs: 3_600,
            session_idle_timeout_secs: 1_800,
            persistent_session_ttl_secs: 86_400,
            persistent_session_idle_timeout_secs: 3_600,
            max_sessions_per_user: 10,
            login_max_failures: 5,
            login_ip_max_failures: 6,
            login_account_ip_max_failures: 5,
            login_failure_window_secs: 900,
            login_lockout_secs: 900,
            trusted_proxy_cidrs: vec!["10.0.0.0/8".parse().expect("trusted proxy CIDR")],
            secure_cookies: false,
        }),
        mfa: Arc::new(
            MfaConfig::new_with_totp_step(
                &[7_u8; 32],
                "localhost",
                "http://localhost:4200",
                "Arc Admin",
                MFA_TEST_STEP_SECS,
            )
            .expect("MFA config"),
        ),
        supervisor: Arc::new(HarnessSupervisor::new(
            pool.clone(),
            "codex".to_string(),
            1,
            60,
            "/tmp/devrail-test".to_string(),
            1,
            Default::default(),
        )),
        run_workspace_root: Arc::new(std::path::PathBuf::from("/tmp/devrail-workspaces")),
        web_push_public_key: None,
    })
    .layer(MockConnectInfo(
        "10.2.0.5:4567"
            .parse::<SocketAddr>()
            .expect("mock peer address"),
    ));
    ApiTestFixture { schema, pool, app }
}

async fn admin_fixture() -> (ApiTestFixture, Value, TestSession) {
    let fixture = api_test_fixture().await;
    services::auth::bootstrap_super_admin(
        &fixture.pool,
        "admin",
        "integration-admin-password",
        "Integration Administrator",
        Some("admin@example.test".to_string()),
    )
    .await
    .expect("bootstrap test administrator");
    let (status, admin_login, token) =
        login(&fixture.app, "admin", "integration-admin-password", false).await;
    assert_eq!(status, StatusCode::OK);
    let token = token.expect("administrator session cookies");
    (fixture, admin_login, token)
}

async fn permission_id(pool: &sqlx::PgPool, code: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM permissions WHERE code = $1")
        .bind(code)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("permission {code} lookup failed: {error}"))
}

#[tokio::test]
async fn authentication_and_mfa_flow() {
    let fixture = api_test_fixture().await;
    let ApiTestFixture { schema, pool, app } = fixture;

    let metrics_response = request(&app, Method::GET, "/metrics", None, None, false, None).await;
    assert_eq!(metrics_response.status(), StatusCode::OK);
    assert_eq!(
        metrics_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .expect("collect metrics body")
        .to_bytes();
    let metrics_body = String::from_utf8(metrics_body.to_vec()).expect("metrics are UTF-8");
    assert!(metrics_body.contains("arc_admin_db_pool_size"));

    let (status, health) = send(&app, Method::GET, "/api/v1/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");

    let (status, readiness) = send(&app, Method::GET, "/api/v1/readyz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(readiness["db"], true);

    let (status, _) = send(&app, Method::GET, "/api/v1/users", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for attempt in 1..=5 {
        let (status, _) = send_from_ip(
            &app,
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({
                "username": "rate_limited_missing_user",
                "password": "incorrect-password"
            })),
            "198.51.100.20",
        )
        .await;
        let expected = if attempt < 5 {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::TOO_MANY_REQUESTS
        };
        assert_eq!(status, expected);
    }
    for username in ["another_missing_user", "third_missing_user"] {
        let (status, _) = send_from_ip(
            &app,
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({
                "username": username,
                "password": "incorrect-password"
            })),
            "198.51.100.20",
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }
    let (status, _) = send_from_ip(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({
            "username": "malformed_forwarded_user",
            "password": "incorrect-password"
        })),
        "198.51.100.21, invalid",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "畸形转发链必须归因到可解析客户端，不能落入已锁定的代理共享桶"
    );

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username": "admin", "password": "admin123"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    services::auth::bootstrap_super_admin(
        &pool,
        "admin",
        "integration-admin-password",
        "Integration Administrator",
        Some("admin@example.test".to_string()),
    )
    .await
    .expect("bootstrap test administrator");

    let password_stage = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "admin",
            "password": "integration-admin-password",
            "remember": false,
        })),
        false,
        None,
    )
    .await;
    assert!(password_stage
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .all(|cookie| !cookie.starts_with("arc_session=")));
    let (status, password_stage_body) = response_json(password_stage).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(password_stage_body["status"], "mfaEnrollmentRequired");

    let (status, admin_login, token) =
        login(&app, "admin", "integration-admin-password", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(admin_login.get("accessToken").is_none());
    assert_eq!(admin_login["status"], "authenticated");
    assert_eq!(
        admin_login["recoveryCodes"]
            .as_array()
            .expect("new recovery codes")
            .len(),
        10
    );
    let first_recovery_code = admin_login["recoveryCodes"][0]
        .as_str()
        .expect("recovery code");
    let stored_recovery_hash = sqlx::query_scalar::<_, String>(
        "SELECT code_hash FROM user_mfa_recovery_codes ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("stored recovery code hash");
    assert_ne!(stored_recovery_hash, first_recovery_code);
    let token = token.expect("session cookies");
    assert!(token.session_set_cookie.contains("HttpOnly"));
    assert!(token.session_set_cookie.contains("SameSite=Strict"));
    assert!(!token.csrf_set_cookie.contains("HttpOnly"));
    assert!(token.csrf_set_cookie.contains("SameSite=Strict"));

    let (status, unauthenticated_repairs) =
        send(&app, Method::GET, "/api/v1/repairs", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(unauthenticated_repairs["error"]["code"], "UNAUTHORIZED");

    let (status, invalid_repair) = send(
        &app,
        Method::POST,
        "/api/v1/runs/1/repairs",
        Some(&token),
        Some(json!({
            "idempotencyKey": "invalid key",
            "riskCategory": "low_risk"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid_repair["error"]["code"], "VALIDATION_ERROR");

    let (status, repair_conflict) = send(
        &app,
        Method::POST,
        "/api/v1/repairs/9223372036854775807/cancel",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(repair_conflict["error"]["code"], "CONFLICT");

    let (status, missing_repair) = send(
        &app,
        Method::GET,
        "/api/v1/repairs/9223372036854775807",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing_repair["error"]["code"], "NOT_FOUND");
    let (status, mfa_status) =
        send(&app, Method::GET, "/api/v1/auth/me/mfa", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mfa_status["required"], true);
    assert_eq!(mfa_status["totpEnabled"], true);
    assert_eq!(mfa_status["recoveryCodesRemaining"], 10);

    let admin_user_id = admin_login["user"]["id"].as_i64().expect("admin user id");
    let (status, locked_status) = send(
        &app,
        Method::GET,
        "/api/v1/auth/me/module-unlocks/users",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(locked_status["unlocked"], false);
    let locked_write = request(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{admin_user_id}"),
        Some(&token),
        Some(json!({"displayName": "Integration Administrator"})),
        true,
        None,
    )
    .await;
    assert_eq!(locked_write.status(), StatusCode::FORBIDDEN);

    issue_module_unlock(&app, &token, "users").await;
    for _ in 0..2 {
        let unlocked_write = request(
            &app,
            Method::PUT,
            &format!("/api/v1/users/{admin_user_id}"),
            Some(&token),
            Some(json!({"displayName": "Integration Administrator"})),
            true,
            None,
        )
        .await;
        assert_eq!(unlocked_write.status(), StatusCode::OK);
    }

    let (status, mfa_challenge) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "admin",
            "password": "integration-admin-password",
            "remember": false,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mfa_challenge["status"], "mfaRequired");
    for attempt in 1..=5 {
        let (status, _) = send(
            &app,
            Method::POST,
            "/api/v1/auth/mfa/totp/verify",
            None,
            Some(json!({
                "challengeToken": mfa_challenge["challengeToken"],
                "code": "无效验证码",
            })),
        )
        .await;
        assert_eq!(
            status,
            if attempt < 5 {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
    let stored_session_hash = sqlx::query_scalar::<_, String>(
        "SELECT session_token_hash FROM auth_sessions ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("stored session hash");
    assert_eq!(
        stored_session_hash.trim(),
        auth::token_hash(&token.session_token)
    );
    assert_ne!(stored_session_hash.trim(), token.session_token);
    let token = &token;

    let (status, csrf_error) = response_json(
        request(
            &app,
            Method::PUT,
            "/api/v1/auth/me/password",
            Some(token),
            Some(json!({
                "currentPassword": "integration-admin-password",
                "newPassword": "updated-integration-admin-password"
            })),
            false,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(csrf_error["error"]["code"], "CSRF_INVALID");

    let (status, _) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        None,
        Some(json!({
            "currentPassword": "integration-admin-password",
            "newPassword": "updated-integration-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, error) = response_json(
        request(
            &app,
            Method::PUT,
            "/api/v1/auth/me/password",
            Some(token),
            Some(json!({
                "currentPassword": "integration-admin-password",
                "newPassword": "updated-integration-admin-password"
            })),
            true,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["message"], "需要先完成身份再认证");

    let (status, error) = send(
        &app,
        Method::POST,
        "/api/v1/auth/me/step-up",
        Some(token),
        Some(json!({
            "currentPassword": "incorrect-password",
            "totpCode": "invalid",
            "scope": "auth.password.change"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["message"], "当前密码不正确");

    let (status, error) = send(
        &app,
        Method::POST,
        "/api/v1/auth/me/step-up",
        Some(token),
        Some(json!({
            "currentPassword": "integration-admin-password",
            "totpCode": "invalid",
            "scope": "auth.password.change"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["message"], "身份验证器验证码不正确");
    let (status, _) = send(&app, Method::GET, "/api/v1/auth/me", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);

    let (replay_token, replayed_totp_code) =
        issue_step_up_with_code(&app, token, "auth.password.change").await;
    let (status, replay_error) = send(
        &app,
        Method::POST,
        "/api/v1/auth/me/step-up",
        Some(token),
        Some(json!({
            "currentPassword": "integration-admin-password",
            "totpCode": replayed_totp_code,
            "scope": "auth.password.change"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        replay_error["error"]["message"],
        "验证码已使用或已过期，请输入最新验证码"
    );
    let (status, _) = response_json(
        request_with_step_up(
            &app,
            Method::PUT,
            "/api/v1/auth/me/password",
            token,
            Some(json!({
                "currentPassword": "integration-admin-password",
                "newPassword": "short"
            })),
            &replay_token,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, error) = response_json(
        request_with_step_up(
            &app,
            Method::PUT,
            "/api/v1/auth/me/password",
            token,
            Some(json!({
                "currentPassword": "integration-admin-password",
                "newPassword": "updated-integration-admin-password"
            })),
            &replay_token,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["message"], "再认证凭据已失效或已使用");

    let (status, error) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        Some(token),
        Some(json!({
            "currentPassword": "integration-admin-password",
            "newPassword": "short"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["message"], "密码长度需在 12-128 个字符之间");

    let (status, error) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        Some(token),
        Some(json!({
            "currentPassword": "integration-admin-password",
            "newPassword": "integration-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["message"], "新密码不能与当前密码相同");

    let (status, error) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        Some(token),
        Some(json!({
            "currentPassword": "incorrect-password",
            "newPassword": "updated-integration-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["message"], "当前密码不正确");

    let (status, _) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        Some(token),
        Some(json!({
            "currentPassword": "integration-admin-password",
            "newPassword": "updated-integration-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, Method::GET, "/api/v1/auth/me", Some(token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "admin",
            "password": "integration-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _refreshed_login, token) = login_with_totp_secret(
        &app,
        "admin",
        "updated-integration-admin-password",
        false,
        token.totp_secret.as_deref(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = token.expect("refreshed session cookies");
    let token = &token;

    let (status, _, replayed_session) = login_with_totp_secret(
        &app,
        "admin",
        "updated-integration-admin-password",
        true,
        token.totp_secret.as_deref(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(replayed_session.is_none());

    tokio::time::sleep(std::time::Duration::from_secs(MFA_TEST_STEP_SECS)).await;
    let (status, _, logout_session) = login_with_totp_secret(
        &app,
        "admin",
        "updated-integration-admin-password",
        true,
        token.totp_secret.as_deref(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logout_session = logout_session.expect("persistent session cookies");
    assert!(logout_session.session_set_cookie.contains("Max-Age="));
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(&logout_session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/auth/me",
        Some(&logout_session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    drop(app);
    schema
        .cleanup()
        .await
        .expect("cleanup authentication schema");
}

#[tokio::test]
async fn organization_and_permissions_flow() {
    let (fixture, _admin_login, token) = admin_fixture().await;
    let ApiTestFixture { schema, app, .. } = fixture;
    let token = &token;

    let (status, permission_groups) = send(
        &app,
        Method::GET,
        "/api/v1/permissions/groups",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let groups = permission_groups.as_array().expect("permission groups");
    assert_eq!(groups[0]["name"], "仪表盘模块");
    assert_eq!(groups[1]["name"], "身份与访问模块");
    assert_eq!(groups[2]["name"], "审计与合规模块");
    assert_eq!(groups[3]["name"], "DevRail Harness");
    let group_codes = groups
        .iter()
        .filter_map(|group| group["code"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        group_codes,
        vec!["dashboard", "identity", "audit", "devrail", "organization"]
    );
    let view_permissions = groups
        .iter()
        .flat_map(|group| group["permissions"].as_array().into_iter().flatten())
        .find(|permission| permission["code"] == "permission:directory:read")
        .expect("view permissions entry");
    assert_eq!(view_permissions["name"], "查看权限");
    let permission_codes = groups
        .iter()
        .flat_map(|group| {
            group["permissions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|permission| permission["code"].as_str())
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        permission_codes,
        BTreeSet::from([
            "dashboard:analytics:read",
            "audit:logs:read",
            "organization:department:read",
            "organization:department:write",
            "permission:directory:read",
            "role:directory:read",
            "role:permissions:write",
            "role:write",
            "user:admin:deactivate",
            "user:admin:reset_password",
            "user:directory:read",
            "user:roles:write",
            "user:super_admin:grant",
            "user:write",
            "devrail:project:read",
            "devrail:project:write",
            "devrail:repository:read",
            "devrail:repository:write",
            "devrail:environment:read",
            "devrail:environment:write",
            "devrail:task:read",
            "devrail:task:write",
            "devrail:task_dependency:read",
            "devrail:task_dependency:write",
            "devrail:followup:create",
            "devrail:continuation:read",
            "devrail:continuation:create",
            "devrail:continuation:cancel",
            "devrail:repair:read",
            "devrail:repair:create",
            "devrail:repair:cancel",
            "devrail:repair:approve",
            "devrail:repair:handoff",
            "devrail:artifact:read",
            "devrail:comment:read",
            "devrail:comment:write",
            "devrail:run:read",
            "devrail:run:execute",
            "devrail:run:interrupt",
            "devrail:run:retry",
            "devrail:approval:read",
            "devrail:approval:approve",
            "devrail:approval:reject",
            "devrail:review:read",
            "devrail:review:write",
            "devrail:member:read",
            "devrail:member:write",
            "devrail:notification:read",
            "devrail:notification:write",
            "devrail:push_device:read",
            "devrail:push_device:write",
            "devrail:push_device:revoke",
            "devrail:workspace:read",
            "devrail:workspace:write",
        ])
    );
    let (status, departments) =
        send(&app, Method::GET, "/api/v1/departments", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let root_department_id = departments[0]["id"].as_i64().expect("root department id");
    assert_eq!(departments[0]["name"], "根部门");
    assert_eq!(departments[0]["depth"], 0);

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/departments/{root_department_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, engineering) = send(
        &app,
        Method::POST,
        "/api/v1/departments",
        Some(token),
        Some(json!({
            "parentId": root_department_id,
            "code": "engineering",
            "name": "研发中心"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let engineering_id = engineering["id"].as_i64().expect("engineering id");
    assert_eq!(engineering["depth"], 1);

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/departments",
        Some(token),
        Some(json!({
            "parentId": root_department_id,
            "code": "engineering",
            "name": "重复研发中心"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, platform) = send(
        &app,
        Method::POST,
        "/api/v1/departments",
        Some(token),
        Some(json!({
            "parentId": engineering_id,
            "code": "platform",
            "name": "平台部"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let platform_id = platform["id"].as_i64().expect("platform id");
    assert_eq!(platform["depth"], 2);

    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/departments/{engineering_id}"),
        Some(token),
        Some(json!({"status": "inactive"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, updated_platform) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/departments/{platform_id}"),
        Some(token),
        Some(json!({"parentId": engineering_id, "name": "平台部（已调整）"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_platform["name"], "平台部（已调整）");
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/departments/{engineering_id}"),
        Some(token),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/departments/{engineering_id}"),
        Some(token),
        Some(json!({"parentId": platform_id})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, department_member) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "department_member",
            "password": "integration-pass",
            "displayName": "部门成员",
            "departmentId": engineering_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(department_member["departmentId"], engineering_id);
    let department_member_id = department_member["id"]
        .as_i64()
        .expect("department member id");

    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{department_member_id}"),
        Some(token),
        Some(json!({"departmentId": platform_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, departments) =
        send(&app, Method::GET, "/api/v1/departments", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let engineering = departments
        .as_array()
        .expect("department array")
        .iter()
        .find(|department| department["id"] == engineering_id)
        .expect("engineering department");
    let platform = departments
        .as_array()
        .expect("department array")
        .iter()
        .find(|department| department["id"] == platform_id)
        .expect("platform department");
    assert_eq!(engineering["childCount"], 1);
    assert_eq!(engineering["memberCount"], 0);
    assert_eq!(platform["memberCount"], 1);

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/departments/{engineering_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/users/{department_member_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for department_id in [platform_id, engineering_id] {
        let (status, _) = send(
            &app,
            Method::DELETE,
            &format!("/api/v1/departments/{department_id}"),
            Some(token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    drop(app);
    schema.cleanup().await.expect("cleanup organization schema");
}

#[tokio::test]
async fn devrail_resources_and_audit_flow() {
    let (fixture, admin_login, token) = admin_fixture().await;
    let ApiTestFixture { schema, pool, app } = fixture;
    let token = &token;
    let user_read_id = permission_id(&pool, "user:directory:read").await;
    let user_write_id = permission_id(&pool, "user:write").await;
    let user_roles_write_id = permission_id(&pool, "user:roles:write").await;
    let role_read_id = permission_id(&pool, "role:directory:read").await;
    let role_write_id = permission_id(&pool, "role:write").await;
    let role_permission_write_id = permission_id(&pool, "role:permissions:write").await;
    let permission_read_id = permission_id(&pool, "permission:directory:read").await;
    let first_recovery_code = admin_login["recoveryCodes"][0]
        .as_str()
        .expect("administrator recovery code")
        .to_string();

    let (status, roles) = send(&app, Method::GET, "/api/v1/roles", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let viewer_role = roles
        .as_array()
        .expect("roles array")
        .iter()
        .find(|role| role["code"] == "viewer")
        .expect("viewer role");
    assert_eq!(viewer_role["name"], "查看者");
    assert_eq!(viewer_role["category"], "只读");
    let viewer_role_id = viewer_role["id"].as_i64().expect("viewer role id");
    let super_admin_role_id = roles
        .as_array()
        .expect("roles array")
        .iter()
        .find(|role| role["code"] == "super_admin")
        .and_then(|role| role["id"].as_i64())
        .expect("super admin role id");
    let support_role_id = roles
        .as_array()
        .expect("roles array")
        .iter()
        .find(|role| role["code"] == "support_tier2")
        .and_then(|role| role["id"].as_i64())
        .expect("support role id");
    assert_eq!(
        roles
            .as_array()
            .expect("roles array")
            .iter()
            .find(|role| role["code"] == "super_admin")
            .expect("super admin role")["dataScope"],
        "all"
    );
    assert_eq!(viewer_role["dataScope"], "self");

    let isolated_org_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO organizations (code, name) VALUES ('isolated', '隔离组织') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("create isolated organization");
    let isolated_department_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO departments (organization_id, code, name)
         VALUES ($1, 'root', '隔离根部门') RETURNING id",
    )
    .bind(isolated_org_id)
    .fetch_one(&pool)
    .await
    .expect("create isolated department");
    let isolated_password = services::auth::hash_password("isolated-integration-pass")
        .expect("hash isolated user password");
    let isolated_user_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (
             username, password_hash, display_name, status, organization_id, department_id
         ) VALUES ('isolated_support', $1, '隔离支持人员', 'active', $2, $3)
         RETURNING id",
    )
    .bind(isolated_password)
    .bind(isolated_org_id)
    .bind(isolated_department_id)
    .fetch_one(&pool)
    .await
    .expect("create isolated user");
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)")
        .bind(isolated_user_id)
        .bind(support_role_id)
        .execute(&pool)
        .await
        .expect("assign isolated support role");
    let (status, _, isolated_token) =
        login(&app, "isolated_support", "isolated-integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let isolated_token = isolated_token.expect("isolated session cookies");
    let (status, isolated_users) = send(
        &app,
        Method::GET,
        "/api/v1/users",
        Some(&isolated_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(isolated_users["total"], 1);
    assert_eq!(isolated_users["items"][0]["username"], "isolated_support");

    let (status, allocator_role) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({
            "code": "role_allocator",
            "name": "角色分配员",
            "permissionIds": [user_read_id, user_write_id, user_roles_write_id, role_read_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let allocator_role_id = allocator_role["id"].as_i64().expect("allocator role id");
    let (status, allocator) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "role_allocator",
            "password": "integration-pass",
            "displayName": "Role Allocator",
            "roleIds": [allocator_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let allocator_id = allocator["id"].as_i64().expect("allocator user id");
    let (status, _, allocator_token) =
        login(&app, "role_allocator", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let allocator_token = allocator_token.expect("allocator session cookies");
    let allocator_token = &allocator_token;
    let (status, allocator_users) = send(
        &app,
        Method::GET,
        "/api/v1/users",
        Some(allocator_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(allocator_users["total"], 1);
    assert_eq!(allocator_users["items"][0]["id"], allocator_id);
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{allocator_id}/roles"),
        Some(allocator_token),
        Some(json!({"roleIds": [super_admin_role_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, support) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "support_actor",
            "password": "integration-pass",
            "displayName": "Support Actor",
            "roleIds": [support_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let support_id = support["id"].as_i64().expect("support user id");
    let (status, inactive_role) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({"code": "inactive_assignment", "name": "停用分配测试角色"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let inactive_role_id = inactive_role["id"].as_i64().expect("inactive role id");
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{inactive_role_id}"),
        Some(token),
        Some(json!({"isActive": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, error) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{support_id}/roles"),
        Some(token),
        Some(json!({"roleIds": [inactive_role_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error["error"]["message"],
        "角色列表中包含不存在或已停用的角色"
    );
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{support_id}/roles"),
        Some(token),
        Some(json!({"roleIds": [support_role_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, support_token) = login(&app, "support_actor", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let support_token = support_token.expect("support session cookies");
    let support_token = &support_token;
    let (status, isolated_lookup) = send(
        &app,
        Method::GET,
        "/api/v1/users?keyword=isolated_support",
        Some(support_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(isolated_lookup["total"], 0);
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{support_id}/roles"),
        Some(support_token),
        Some(json!({"roleIds": [super_admin_role_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, suspended_target) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "suspended_target",
            "password": "integration-pass",
            "displayName": "Suspended Target",
            "status": "suspended"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let suspended_target_id = suspended_target["id"]
        .as_i64()
        .expect("suspended target id");
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{suspended_target_id}"),
        Some(support_token),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, role_writer_role) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({
            "code": "role_writer",
            "name": "角色编辑员",
            "permissionIds": [role_read_id, role_write_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let role_writer_role_id = role_writer_role["id"]
        .as_i64()
        .expect("role writer role id");
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{allocator_id}/roles"),
        Some(allocator_token),
        Some(json!({"roleIds": [role_writer_role_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "role_writer",
            "password": "integration-pass",
            "displayName": "Role Writer",
            "roleIds": [role_writer_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, role_writer_token) =
        login(&app, "role_writer", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let role_writer_token = role_writer_token.expect("role writer session cookies");
    let role_writer_token = &role_writer_token;
    let (status, error) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(role_writer_token),
        Some(json!({
            "code": "global_scope_attempt",
            "name": "Global Scope Attempt",
            "dataScope": "all"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["message"], "不能授予超出自身范围的数据权限");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(role_writer_token),
        Some(json!({
            "code": "privileged_role",
            "name": "Privileged Role",
            "permissionIds": [user_write_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, permission_manager_role) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({
            "code": "permission_manager",
            "name": "权限分配员",
            "permissionIds": [role_read_id, role_permission_write_id, permission_read_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let permission_manager_role_id = permission_manager_role["id"]
        .as_i64()
        .expect("permission manager role id");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "permission_manager",
            "password": "integration-pass",
            "displayName": "Permission Manager",
            "roleIds": [permission_manager_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, permission_manager_token) =
        login(&app, "permission_manager", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let permission_manager_token =
        permission_manager_token.expect("permission manager session cookies");
    let permission_manager_token = &permission_manager_token;

    let admin_id = admin_login["user"]["id"]
        .as_i64()
        .expect("administrator id");
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{admin_id}"),
        Some(token),
        Some(json!({"status": "inactive"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{super_admin_role_id}"),
        Some(token),
        Some(json!({"isActive": false})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{super_admin_role_id}/permissions"),
        Some(token),
        Some(json!({"permissionIds": []})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "rolled_back_user",
            "password": "integration-pass",
            "displayName": "Rolled Back User",
            "roleIds": [i64::MAX]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, users) = send(
        &app,
        Method::GET,
        "/api/v1/users?keyword=rolled_back_user",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users["total"], 0);

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({
            "code": "rolled_back_role",
            "name": "Rolled Back Role",
            "permissionIds": [i64::MAX]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, roles_after_failure) =
        send(&app, Method::GET, "/api/v1/roles", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(roles_after_failure
        .as_array()
        .expect("roles array")
        .iter()
        .all(|role| role["code"] != "rolled_back_role"));

    let (status, created) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "integration_user",
            "password": "integration-pass",
            "displayName": "Integration User",
            "email": "integration@example.com",
            "roleIds": [viewer_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let user_id = created["id"].as_i64().expect("created user id");
    assert_eq!(created["roles"], json!(["查看者"]));

    let (status, filtered_users) = send(
        &app,
        Method::GET,
        "/api/v1/users?keyword=integration&role=%E6%9F%A5%E7%9C%8B%E8%80%85&sortBy=username&sortDirection=desc&page=99&pageSize=1",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(filtered_users["total"], 1);
    assert_eq!(filtered_users["page"], 1);
    assert_eq!(filtered_users["items"][0]["username"], "integration_user");
    assert!(filtered_users["roleOptions"]
        .as_array()
        .expect("role options")
        .contains(&json!("查看者")));

    let (status, invalid_sort) = send(
        &app,
        Method::GET,
        "/api/v1/users?sortBy=passwordHash&sortDirection=asc",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(invalid_sort["error"]["message"]
        .as_str()
        .expect("sort validation message")
        .contains("sortBy"));

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/roles/{viewer_role_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, updated) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{user_id}"),
        Some(token),
        Some(json!({"email": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated["email"].is_null());

    let (status, clearable_role) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(token),
        Some(json!({
            "code": "clearable_role",
            "name": "Clearable Role",
            "icon": "badge",
            "description": "Temporary description"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let clearable_role_id = clearable_role["id"].as_i64().expect("clearable role id");
    let (status, error) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{clearable_role_id}/permissions"),
        Some(token),
        Some(json!({"permissionIds": [role_permission_write_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error["error"]["message"],
        "分配角色权限管理能力时，必须同时授予角色读取和权限目录读取权限"
    );
    let (status, error) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{clearable_role_id}/permissions"),
        Some(permission_manager_token),
        Some(json!({
            "permissionIds": [
                role_read_id,
                role_permission_write_id,
                permission_read_id,
                user_write_id
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["message"], "不能授予操作者自身未拥有的权限");
    let (status, clearable_role) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{clearable_role_id}"),
        Some(token),
        Some(json!({"icon": null, "description": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(clearable_role["icon"].is_null());
    assert!(clearable_role["description"].is_null());

    let (status, _, viewer_token) =
        login(&app, "integration_user", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let viewer_token = viewer_token.expect("viewer session cookies");
    let viewer_token = &viewer_token;

    let (status, repair_create_forbidden) = send(
        &app,
        Method::POST,
        "/api/v1/runs/1/repairs",
        Some(viewer_token),
        Some(json!({
            "idempotencyKey": "viewer-repair-request",
            "riskCategory": "low_risk"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(repair_create_forbidden["error"]["code"], "FORBIDDEN");

    let (status, deactivated_role) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{viewer_role_id}"),
        Some(token),
        Some(json!({"isActive": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deactivated_role["isActive"], false);
    assert_eq!(deactivated_role["members"], 1);

    let (status, permissions_without_role) = send(
        &app,
        Method::GET,
        "/api/v1/auth/me/permissions",
        Some(viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(permissions_without_role["codes"], json!([]));

    let (status, reactivated_role) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/roles/{viewer_role_id}"),
        Some(token),
        Some(json!({"isActive": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reactivated_role["isActive"], true);
    assert_eq!(reactivated_role["members"], 1);

    let (status, restored_permissions) = send(
        &app,
        Method::GET,
        "/api/v1/auth/me/permissions",
        Some(viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(restored_permissions["codes"]
        .as_array()
        .expect("permission codes")
        .iter()
        .any(|code| code == "role:directory:read"));

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/roles",
        Some(viewer_token),
        Some(json!({"code": "forbidden_role", "name": "Forbidden Role"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, updated) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{user_id}"),
        Some(token),
        Some(json!({"status": "inactive"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["status"], "inactive");

    let (status, _) = send(&app, Method::GET, "/api/v1/roles", Some(viewer_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "integration_user",
            "password": "integration-pass"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, users) = send(
        &app,
        Method::GET,
        "/api/v1/users?keyword=integration_user",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users["total"], 1);

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/users/{user_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/users/{user_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, roles_after_user_delete) =
        send(&app, Method::GET, "/api/v1/roles", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let viewer_after_delete = roles_after_user_delete
        .as_array()
        .expect("roles array")
        .iter()
        .find(|role| role["code"] == "viewer")
        .expect("viewer role");
    assert_eq!(viewer_after_delete["members"], 0);

    let (status, second_admin) = send(
        &app,
        Method::POST,
        "/api/v1/users",
        Some(token),
        Some(json!({
            "username": "second_admin",
            "password": "integration-pass",
            "displayName": "Second Administrator",
            "roleIds": [super_admin_role_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_admin_id = second_admin["id"].as_i64().expect("second admin id");

    let (status, _, second_admin_session) =
        login(&app, "second_admin", "integration-pass", false).await;
    assert_eq!(status, StatusCode::OK);
    let second_admin_session = second_admin_session.expect("second admin session cookies");
    let second_admin_totp_secret = second_admin_session.totp_secret.clone();
    let (status, _) = send(
        &app,
        Method::PUT,
        "/api/v1/auth/me/password",
        Some(&second_admin_session),
        Some(json!({
            "currentPassword": "integration-pass",
            "newPassword": "updated-second-admin-password"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, refreshed_second_admin) = login_with_totp_secret(
        &app,
        "second_admin",
        "updated-second-admin-password",
        false,
        second_admin_totp_secret.as_deref(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let refreshed_second_admin = refreshed_second_admin.expect("refreshed second admin session");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(&refreshed_second_admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, error) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/users/{admin_id}"),
        Some(token),
        Some(json!({"status": "inactive"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["message"], "不能停用当前登录账号");

    let (status, audit_logs) = send(
        &app,
        Method::GET,
        "/api/v1/audit-logs?page=1&pageSize=100&action=user.roles.update",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(audit_logs["total"].as_i64().unwrap_or_default() > 0);
    assert!(audit_logs["items"]
        .as_array()
        .expect("audit log items")
        .iter()
        .all(|entry| entry["action"] == "user.roles.update"));
    assert!(audit_logs["items"]
        .as_array()
        .expect("audit log items")
        .iter()
        .all(|entry| entry["traceId"]
            .as_str()
            .is_some_and(|trace_id| !trace_id.is_empty())));
    let (status, first_audit_page) = send(
        &app,
        Method::GET,
        "/api/v1/audit-logs?page=1&pageSize=1",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first_audit_id = first_audit_page["items"][0]["id"]
        .as_i64()
        .expect("first audit id");
    let next_cursor = first_audit_page["nextCursor"]
        .as_str()
        .expect("audit cursor");
    let (status, second_audit_page) = send(
        &app,
        Method::GET,
        &format!("/api/v1/audit-logs?page=2&pageSize=1&cursor={next_cursor}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(
        second_audit_page["items"][0]["id"].as_i64(),
        Some(first_audit_id)
    );
    let trace_id = audit_logs["items"][0]["traceId"]
        .as_str()
        .expect("audit trace id");
    let (status, traced_audit_logs) = send(
        &app,
        Method::GET,
        &format!("/api/v1/audit-logs?keyword={trace_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(traced_audit_logs["items"]
        .as_array()
        .expect("traced audit log items")
        .iter()
        .all(|entry| entry["traceId"] == trace_id));

    let (status, revocation_logs) = send(
        &app,
        Method::GET,
        "/api/v1/audit-logs?page=1&pageSize=100&action=auth.session.revoked",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let revocation_reasons = revocation_logs["items"]
        .as_array()
        .expect("session revocation audit items")
        .iter()
        .filter_map(|entry| entry["details"]["reason"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(revocation_reasons.contains("password_change"));
    assert!(revocation_reasons.contains("logout"));
    assert!(revocation_reasons.contains("account_status_changed"));

    let (status, login_audit_logs) = send(
        &app,
        Method::GET,
        "/api/v1/audit-logs?page=1&pageSize=100&action=auth.login.success",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(login_audit_logs["items"]
        .as_array()
        .expect("login audit items")
        .iter()
        .all(|entry| entry["details"]["sourceIpFingerprint"]
            .as_str()
            .is_some_and(|fingerprint| fingerprint.len() == 12)));

    let (status, recovery_challenge) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "admin",
            "password": "integration-admin-password",
            "remember": false,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recovery_login = request(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/recovery/verify",
        None,
        Some(json!({
            "challengeToken": recovery_challenge["challengeToken"],
            "code": first_recovery_code,
        })),
        false,
        None,
    )
    .await;
    assert!(recovery_login
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|cookie| cookie.starts_with("arc_session=")));
    let (status, recovery_login_body) = response_json(recovery_login).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovery_login_body["status"], "authenticated");

    let (status, replay_challenge) = send(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({
            "username": "admin",
            "password": "integration-admin-password",
            "remember": false,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/auth/mfa/recovery/verify",
        None,
        Some(json!({
            "challengeToken": replay_challenge["challengeToken"],
            "code": first_recovery_code,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let used_recovery_codes = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_mfa_recovery_codes WHERE used_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("used recovery code count");
    assert_eq!(used_recovery_codes, 1);

    let deactivate = || UpdateUserRequest {
        display_name: None,
        email: NullablePatch::Missing,
        status: Some("inactive".to_string()),
        department_id: None,
        password: None,
    };
    let first_request = deactivate();
    let second_request = deactivate();
    let (first, second) = tokio::join!(
        services::users::update(&pool, admin_id, None, &first_request, false, false),
        services::users::update(&pool, second_admin_id, None, &second_request, false, false),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);

    sqlx::query(
        "UPDATE roles SET name = '自定义查看角色', category = 'Read Only' WHERE code = 'viewer'",
    )
    .execute(&pool)
    .await
    .expect("prepare customized role");
    sqlx::raw_sql(include_str!(
        "../migrations/0007_localize_default_copy_zh_cn.sql"
    ))
    .execute(&pool)
    .await
    .expect("rerun localization migration");
    let localized_role = sqlx::query_as::<_, (String, String)>(
        "SELECT name, category FROM roles WHERE code = 'viewer'",
    )
    .fetch_one(&pool)
    .await
    .expect("localized viewer role");
    assert_eq!(localized_role.0, "自定义查看角色");
    assert_eq!(localized_role.1, "只读");
    sqlx::query("UPDATE roles SET name = '查看者', category = '只读' WHERE code = 'viewer'")
        .execute(&pool)
        .await
        .expect("restore viewer role after localization test");

    let archived_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO audit_logs
             (actor_user_id, action, target_type, target_id, details, created_at)
         VALUES ($1, 'test.archive', 'user', $1, '{}'::jsonb, now() - interval '400 days')
         RETURNING id",
    )
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .expect("insert expired audit row");
    let update_error = sqlx::query("UPDATE audit_logs SET action = 'tampered' WHERE id = $1")
        .bind(archived_id)
        .execute(&pool)
        .await
        .expect_err("audit row update must be blocked");
    let update_error_code = update_error
        .as_database_error()
        .and_then(|error| error.code());
    assert_eq!(update_error_code.as_deref(), Some("55000"));
    sqlx::query("DELETE FROM audit_logs WHERE id = $1")
        .bind(archived_id)
        .execute(&pool)
        .await
        .expect_err("direct audit row delete must be blocked");

    let cutoff = Utc::now() - Duration::days(365);
    let archive_rows = repositories::audit_logs::archive_batch(&pool, cutoff, 100)
        .await
        .expect("load audit archive batch");
    assert!(archive_rows.iter().any(|row| row.id == archived_id));
    let deleted = repositories::audit_logs::delete_archived(&pool, &[archived_id], cutoff)
        .await
        .expect("delete exported audit row through retention repository");
    assert_eq!(deleted, 1);

    drop(app);
    schema.cleanup().await.expect("cleanup DevRail schema");
}
