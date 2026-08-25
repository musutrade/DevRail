//! arc-admin backend server entry point.

use arc_admin_backend::auth::AuthSessionConfig;
use arc_admin_backend::config::{AppConfig, AppEnvironment, LogFormat};
use arc_admin_backend::mfa::MfaConfig;
use arc_admin_backend::telemetry::{self, TelemetryMetadata};
use arc_admin_backend::workers::harness_supervisor::HarnessSupervisor;
use arc_admin_backend::{build_router_with_metadata_and_cors, db, AppState};
use std::process::ExitCode;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::Instrument;

#[tokio::main]
async fn main() -> ExitCode {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let environment_result = if env_path.is_file() {
        dotenvy::from_path(&env_path)
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("failed to load backend/.env: {error}"))
    } else {
        Ok(())
    };
    let _telemetry_guard = match telemetry::init(LogFormat::bootstrap_from_env()) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("failed to initialize logging: {error}");
            return ExitCode::FAILURE;
        }
    };
    telemetry::install_panic_hook();
    if let Err(error) = environment_result {
        tracing::error!(
            event = "application.configuration_failure",
            error = %format!("{error:#}"),
            "application configuration failed"
        );
        return ExitCode::FAILURE;
    }

    let config = match AppConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(
                event = "application.configuration_failure",
                error = %format!("{error:#}"),
                "application configuration failed"
            );
            return ExitCode::FAILURE;
        }
    };
    let metadata = TelemetryMetadata::from_config(&config);
    let application_span = telemetry::application_span(&metadata);

    let result = async move {
        let result = run(config, metadata).await;
        if let Err(error) = &result {
            tracing::error!(
                event = "application.failure",
                error = %format!("{error:#}"),
                "application stopped with error"
            );
        }
        result
    }
    .instrument(application_span)
    .await;
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn run(config: AppConfig, metadata: TelemetryMetadata) -> anyhow::Result<()> {
    let pool = db::init_pool_with_config(&config.database_url, &config.database_pool).await?;
    if config.auto_migrate {
        let migrations = db::run_migrations(&pool).await?;
        tracing::info!(
            applied_migrations = migrations.applied,
            embedded_migrations = migrations.embedded,
            "database migrations ready"
        );
    } else {
        tracing::info!("automatic database migrations disabled");
    }

    let supervisor = Arc::new(HarnessSupervisor::new(
        pool.clone(),
        config.harness_command.clone(),
        config.harness_max_concurrency,
        config.run_max_duration_secs,
        config.run_workspace_root.clone(),
        config.run_graceful_interrupt_secs,
    ));
    let recovered = supervisor.recover_stale_runs().await?;
    if recovered > 0 {
        tracing::warn!(
            recovered_runs = recovered,
            "stale DevRail Harness runs marked failed after supervisor restart"
        );
    }
    let state = AppState {
        pool,
        auth: Arc::new(AuthSessionConfig {
            session_ttl_secs: config.session_ttl_secs,
            session_idle_timeout_secs: config.session_idle_timeout_secs,
            persistent_session_ttl_secs: config.persistent_session_ttl_secs,
            persistent_session_idle_timeout_secs: config.persistent_session_idle_timeout_secs,
            max_sessions_per_user: config.max_sessions_per_user,
            login_max_failures: config.login_max_failures,
            login_ip_max_failures: config.login_ip_max_failures,
            login_account_ip_max_failures: config.login_account_ip_max_failures,
            login_failure_window_secs: config.login_failure_window_secs,
            login_lockout_secs: config.login_lockout_secs,
            trusted_proxy_cidrs: config.trusted_proxy_cidrs.clone(),
            secure_cookies: config.environment == AppEnvironment::Production,
        }),
        mfa: Arc::new(MfaConfig::new(
            &config.mfa_encryption_key,
            &config.webauthn_rp_id,
            &config.webauthn_rp_origin,
            &config.webauthn_rp_name,
        )?),
        supervisor,
        run_workspace_root: Arc::new(PathBuf::from(&config.run_workspace_root)),
        web_push_public_key: config.web_push_public_key.clone().map(Arc::<str>::from),
    };
    arc_admin_backend::workers::approval_expiry::spawn(
        state.pool.clone(),
        state.supervisor.clone(),
    );
    arc_admin_backend::workers::branch_cleanup::spawn(state.pool.clone());
    arc_admin_backend::workers::notification_dispatcher::spawn(
        state.pool.clone(),
        state.mfa.clone(),
        config.web_push_private_key.clone(),
        config.web_push_subject.clone(),
    );
    let app = build_router_with_metadata_and_cors(state, metadata, config.cors_layer());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install termination handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
