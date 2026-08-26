//! 数据库层：连接池初始化、迁移与健康检查（SQL 允许出现在 db 层，见审计 allowlist）

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[cfg(test)]
pub(crate) static DATABASE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct MigrationStatus {
    pub applied: i64,
    pub embedded: usize,
}

#[derive(Debug, Clone)]
pub struct DatabasePoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub max_lifetime_secs: u64,
    pub statement_timeout_ms: u64,
}

#[derive(Debug, Default)]
pub(crate) struct DatabasePoolConfigValues {
    pub(crate) max_connections: Option<String>,
    pub(crate) min_connections: Option<String>,
    pub(crate) acquire_timeout_secs: Option<String>,
    pub(crate) connect_timeout_secs: Option<String>,
    pub(crate) idle_timeout_secs: Option<String>,
    pub(crate) max_lifetime_secs: Option<String>,
    pub(crate) statement_timeout_ms: Option<String>,
}

impl Default for DatabasePoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            acquire_timeout_secs: 5,
            connect_timeout_secs: 10,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1_800,
            statement_timeout_ms: 30_000,
        }
    }
}

impl DatabasePoolConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_values(DatabasePoolConfigValues {
            max_connections: std::env::var("DB_MAX_CONNECTIONS").ok(),
            min_connections: std::env::var("DB_MIN_CONNECTIONS").ok(),
            acquire_timeout_secs: std::env::var("DB_ACQUIRE_TIMEOUT_SECS").ok(),
            connect_timeout_secs: std::env::var("DB_CONNECT_TIMEOUT_SECS").ok(),
            idle_timeout_secs: std::env::var("DB_IDLE_TIMEOUT_SECS").ok(),
            max_lifetime_secs: std::env::var("DB_MAX_LIFETIME_SECS").ok(),
            statement_timeout_ms: std::env::var("DB_STATEMENT_TIMEOUT_MS").ok(),
        })
    }

    pub(crate) fn from_values(values: DatabasePoolConfigValues) -> anyhow::Result<Self> {
        let defaults = Self::default();
        let config = Self {
            max_connections: positive_u32(
                "DB_MAX_CONNECTIONS",
                values.max_connections,
                defaults.max_connections,
            )?,
            min_connections: nonnegative_u32(
                "DB_MIN_CONNECTIONS",
                values.min_connections,
                defaults.min_connections,
            )?,
            acquire_timeout_secs: positive_u64(
                "DB_ACQUIRE_TIMEOUT_SECS",
                values.acquire_timeout_secs,
                defaults.acquire_timeout_secs,
            )?,
            connect_timeout_secs: positive_u64(
                "DB_CONNECT_TIMEOUT_SECS",
                values.connect_timeout_secs,
                defaults.connect_timeout_secs,
            )?,
            idle_timeout_secs: positive_u64(
                "DB_IDLE_TIMEOUT_SECS",
                values.idle_timeout_secs,
                defaults.idle_timeout_secs,
            )?,
            max_lifetime_secs: positive_u64(
                "DB_MAX_LIFETIME_SECS",
                values.max_lifetime_secs,
                defaults.max_lifetime_secs,
            )?,
            statement_timeout_ms: positive_u64(
                "DB_STATEMENT_TIMEOUT_MS",
                values.statement_timeout_ms,
                defaults.statement_timeout_ms,
            )?,
        };
        if config.min_connections > config.max_connections {
            anyhow::bail!("DB_MIN_CONNECTIONS cannot exceed DB_MAX_CONNECTIONS");
        }
        Ok(config)
    }
}

pub async fn init_pool(database_url: &str) -> anyhow::Result<PgPool> {
    init_pool_with_config(database_url, &DatabasePoolConfig::default()).await
}

pub async fn init_pool_with_config(
    database_url: &str,
    config: &DatabasePoolConfig,
) -> anyhow::Result<PgPool> {
    let statement_timeout = format!("{}ms", config.statement_timeout_ms);
    let connect = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .idle_timeout(Some(Duration::from_secs(config.idle_timeout_secs)))
        .max_lifetime(Some(Duration::from_secs(config.max_lifetime_secs)))
        .after_connect(move |connection, _metadata| {
            let statement_timeout = statement_timeout.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                    .bind(statement_timeout)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url);
    let pool = tokio::time::timeout(Duration::from_secs(config.connect_timeout_secs), connect)
        .await
        .map_err(|_| anyhow::anyhow!("database connection timed out"))??;
    Ok(pool)
}

fn positive_u32(name: &str, value: Option<String>, default: u32) -> anyhow::Result<u32> {
    let value = value
        .as_deref()
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| anyhow::anyhow!("{name} must be a positive integer"))?
        .unwrap_or(default);
    if value == 0 {
        anyhow::bail!("{name} must be a positive integer");
    }
    Ok(value)
}

fn nonnegative_u32(name: &str, value: Option<String>, default: u32) -> anyhow::Result<u32> {
    value
        .as_deref()
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| anyhow::anyhow!("{name} must be a non-negative integer"))
        .map(|value| value.unwrap_or(default))
}

fn positive_u64(name: &str, value: Option<String>, default: u64) -> anyhow::Result<u64> {
    let value = value
        .as_deref()
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|_| anyhow::anyhow!("{name} must be a positive integer"))?
        .unwrap_or(default);
    if value == 0 {
        anyhow::bail!("{name} must be a positive integer");
    }
    Ok(value)
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<MigrationStatus> {
    MIGRATOR.run(pool).await?;

    let applied =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = TRUE")
            .fetch_one(pool)
            .await?;

    Ok(MigrationStatus {
        applied,
        embedded: MIGRATOR
            .iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .count(),
    })
}

pub async fn ping(pool: &PgPool) -> bool {
    sqlx::query("SELECT 1").execute(pool).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_configuration_validates_bounds() {
        let error = DatabasePoolConfig::from_values(DatabasePoolConfigValues {
            max_connections: Some("2".to_string()),
            min_connections: Some("3".to_string()),
            ..DatabasePoolConfigValues::default()
        })
        .expect_err("minimum larger than maximum must fail");
        assert!(error.to_string().contains("DB_MIN_CONNECTIONS"));
    }
}
