//! 数据库层：连接池初始化、迁移与健康检查（SQL 允许出现在 db 层，见审计 allowlist）

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[cfg(test)]
/// Serializes tests that still exercise facts in the shared public schema.
pub(crate) static DATABASE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
static TEST_MIGRATIONS_READY: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

#[cfg(test)]
static TEST_MIGRATION_INITIALIZATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

static TEST_SCHEMA_BASE_READY: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static TEST_MIGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static TEST_THREADS_REPORTED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
#[cfg(test)]
static TEST_SERIAL_FACTS_REPORTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

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
    init_pool_with_config_and_search_path(database_url, config, None).await
}

async fn init_pool_with_config_and_search_path(
    database_url: &str,
    config: &DatabasePoolConfig,
    search_path: Option<&str>,
) -> anyhow::Result<PgPool> {
    let statement_timeout = format!("{}ms", config.statement_timeout_ms);
    let search_path = search_path.map(|schema| format!("{}, public", quote_identifier(schema)));
    let connect = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .idle_timeout(Some(Duration::from_secs(config.idle_timeout_secs)))
        .max_lifetime(Some(Duration::from_secs(config.max_lifetime_secs)))
        .after_connect(move |connection, _metadata| {
            let statement_timeout = statement_timeout.clone();
            let search_path = search_path.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                    .bind(statement_timeout)
                    .execute(&mut *connection)
                    .await?;
                if let Some(search_path) = search_path {
                    sqlx::query("SELECT set_config('search_path', $1, false)")
                        .bind(search_path)
                        .execute(&mut *connection)
                        .await?;
                }
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

/// A schema-isolated PostgreSQL fixture for tests that need concurrent database access.
///
/// The management pool remains outside the test schema so cleanup can drop the schema after
/// all schema connections have been returned. Call [`TestSchemaPool::cleanup`] in every test;
/// dropping an uncleaned fixture schedules a best-effort asynchronous cleanup.
#[derive(Debug)]
pub struct TestSchemaPool {
    pool: Option<PgPool>,
    management_pool: Option<PgPool>,
    schema: String,
    cleanup_started: std::sync::atomic::AtomicBool,
    cleanup_completed: std::sync::atomic::AtomicBool,
}

impl TestSchemaPool {
    pub fn pool(&self) -> &PgPool {
        self.pool
            .as_ref()
            .expect("test schema pool is unavailable after cleanup")
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub async fn cleanup(mut self) -> anyhow::Result<()> {
        self.cleanup_started
            .store(true, std::sync::atomic::Ordering::Release);
        let pool = self
            .pool
            .take()
            .expect("test schema pool cleanup was already started");
        let management_pool = self
            .management_pool
            .take()
            .expect("test schema management pool cleanup was already started");
        let schema = self.schema.clone();
        pool.close().await;
        let drop_result = drop_schema(&management_pool, &schema).await;
        management_pool.close().await;
        self.cleanup_completed
            .store(true, std::sync::atomic::Ordering::Release);
        match drop_result {
            Ok(()) => Ok(()),
            Err(error) => {
                eprintln!("TEST_SCHEMA_CLEANUP_FAILED schema={schema} error={error:#}");
                Err(anyhow::anyhow!(
                    "drop test schema {schema} during cleanup failed: {error:#}"
                ))
            }
        }
    }
}

impl Drop for TestSchemaPool {
    fn drop(&mut self) {
        if self
            .cleanup_completed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        if self
            .cleanup_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            eprintln!(
                "TEST_SCHEMA_CLEANUP_INCOMPLETE schema={} cleanup did not finish",
                self.schema
            );
            return;
        }

        let Some(pool) = self.pool.take() else {
            eprintln!(
                "TEST_SCHEMA_CLEANUP_MISSED schema={} test pool was unavailable",
                self.schema
            );
            return;
        };
        let Some(management_pool) = self.management_pool.take() else {
            eprintln!(
                "TEST_SCHEMA_CLEANUP_FAILED schema={} management pool was unavailable",
                self.schema
            );
            return;
        };
        let schema = self.schema.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    pool.close().await;
                    let drop_result = drop_schema(&management_pool, &schema).await;
                    management_pool.close().await;
                    if let Err(error) = drop_result {
                        eprintln!("TEST_SCHEMA_CLEANUP_FAILED schema={schema} error={error:#}");
                    }
                });
            }
            Err(error) => {
                eprintln!(
                    "TEST_SCHEMA_CLEANUP_FAILED schema={} runtime unavailable: {error:#}",
                    self.schema
                );
            }
        }
    }
}

fn panic_after_schema_cleanup(
    stage: &str,
    schema: &str,
    setup_error: anyhow::Error,
    cleanup_result: anyhow::Result<()>,
) -> ! {
    match cleanup_result {
        Ok(()) => panic!("{stage} for schema {schema}: {setup_error:#}"),
        Err(cleanup_error) => panic!(
            "{stage} for schema {schema}: {setup_error:#}; cleanup failed: {cleanup_error:#}"
        ),
    }
}

/// Create a unique schema, run the embedded migrations in it, and configure every pooled
/// connection to resolve unqualified tables from that schema first.
pub async fn test_schema_pool() -> anyhow::Result<TestSchemaPool> {
    report_test_threads();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("TEST_DATABASE_URL is required for schema fixtures"))?;
    let management_config = DatabasePoolConfig {
        max_connections: 1,
        min_connections: 0,
        ..DatabasePoolConfig::default()
    };
    let management_pool = init_pool_with_config(&database_url, &management_config)
        .await
        .unwrap_or_else(|error| {
            panic!("schema test database connection failed: {error:#}");
        });
    let base_database_url = database_url.clone();
    TEST_SCHEMA_BASE_READY
        .get_or_try_init(|| async move {
            let base_pool = init_pool(&base_database_url).await.map_err(|error| {
                anyhow::anyhow!("base test database connection failed: {error:#}")
            })?;
            let migration_result = {
                let _migration_guard = TEST_MIGRATION_LOCK.lock().await;
                run_migrations(&base_pool).await
            };
            base_pool.close().await;
            migration_result
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!("base test database migrations failed: {error:#}"))
        })
        .await
        .unwrap_or_else(|error| {
            panic!("base test database migration initialization failed: {error:#}");
        });
    let schema = format!("devrail_test_{}", Uuid::new_v4().simple());
    if let Err(error) = create_schema(&management_pool, &schema).await {
        management_pool.close().await;
        panic!("test schema creation failed for {schema}: {error:#}");
    }

    let pool = match init_pool_with_config_and_search_path(
        &database_url,
        &DatabasePoolConfig::default(),
        Some(&schema),
    )
    .await
    {
        Ok(pool) => pool,
        Err(error) => {
            let cleanup_result = drop_schema(&management_pool, &schema).await;
            management_pool.close().await;
            panic_after_schema_cleanup(
                "test schema connection initialization failed",
                &schema,
                error,
                cleanup_result,
            );
        }
    };
    let migration_result = {
        let _migration_guard = TEST_MIGRATION_LOCK.lock().await;
        run_migrations(&pool).await
    };
    if let Err(error) = migration_result {
        pool.close().await;
        let cleanup_result = drop_schema(&management_pool, &schema).await;
        management_pool.close().await;
        panic_after_schema_cleanup(
            "test schema migration initialization failed",
            &schema,
            error,
            cleanup_result,
        );
    }

    Ok(TestSchemaPool {
        pool: Some(pool),
        management_pool: Some(management_pool),
        schema,
        cleanup_started: std::sync::atomic::AtomicBool::new(false),
        cleanup_completed: std::sync::atomic::AtomicBool::new(false),
    })
}

async fn create_schema(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
    let statement = format!("CREATE SCHEMA {}", quote_identifier(schema));
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(pool)
        .await?;
    Ok(())
}

async fn drop_schema(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
    let statement = format!("DROP SCHEMA {} CASCADE", quote_identifier(schema));
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(pool)
        .await?;
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub fn report_test_threads() {
    TEST_THREADS_REPORTED.get_or_init(|| {
        let configured = parse_test_threads(std::env::args().skip(1)).or_else(|| {
            std::env::var("DEVRAIL_TEST_THREADS")
                .or_else(|_| std::env::var("RUST_TEST_THREADS"))
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
        });
        let threads = configured
            .or_else(|| std::thread::available_parallelism().ok().map(usize::from))
            .unwrap_or(1);
        eprintln!("DEVRAIL_TEST_THREADS={threads}");
        threads
    });
}

fn parse_test_threads<I>(args: I) -> Option<usize>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if let Some(value) = argument.strip_prefix("--test-threads=") {
            if let Some(threads) = parse_positive_thread_count(value) {
                return Some(threads);
            }
        } else if argument == "--test-threads" {
            if let Some(value) = args.next() {
                if let Some(threads) = parse_positive_thread_count(&value) {
                    return Some(threads);
                }
            }
        }
    }
    None
}

fn parse_positive_thread_count(value: &str) -> Option<usize> {
    value.parse::<usize>().ok().filter(|threads| *threads > 0)
}

#[cfg(test)]
pub(crate) fn report_test_serial_database_facts() {
    TEST_SERIAL_FACTS_REPORTED.get_or_init(|| {
        eprintln!(
            "DEVRAIL_TEST_SERIAL_DATABASE_FACTS=public migration, seed, audit trigger, and recovery-state tests use DATABASE_TEST_LOCK"
        );
    });
}

#[cfg(test)]
pub(crate) async fn test_pool() -> Option<PgPool> {
    report_test_threads();
    report_test_serial_database_facts();
    let database_url = match std::env::var("TEST_DATABASE_URL") {
        Ok(database_url) => database_url,
        Err(_) => return None,
    };
    let migration_database_url = database_url.clone();
    TEST_MIGRATIONS_READY
        .get_or_try_init(|| async move {
            let pool = init_pool(&migration_database_url)
                .await
                .map_err(|error| anyhow::anyhow!("test database connection failed: {error:#}"))?;
            let migration_result = {
                let _migration_guard = TEST_MIGRATION_LOCK.lock().await;
                run_migrations(&pool).await
            };
            pool.close().await;
            migration_result
                .map(|_| {
                    TEST_MIGRATION_INITIALIZATIONS
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                })
                .map_err(|error| anyhow::anyhow!("test database migrations failed: {error:#}"))
        })
        .await
        .unwrap_or_else(|error| {
            panic!("test database migration initialization failed: {error:#}");
        });
    Some(init_pool(&database_url).await.unwrap_or_else(|error| {
        panic!("test database connection failed: {error:#}");
    }))
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

    #[test]
    fn test_thread_parser_accepts_rust_harness_argument_forms() {
        assert_eq!(
            parse_test_threads(vec![
                "--nocapture".to_string(),
                "--test-threads=4".to_string(),
            ]),
            Some(4)
        );
        assert_eq!(
            parse_test_threads(vec!["--test-threads".to_string(), "2".to_string(),]),
            Some(2)
        );
        assert_eq!(
            parse_test_threads(vec!["--test-threads".to_string(), "0".to_string(),]),
            None
        );
    }

    #[tokio::test]
    async fn test_migrations_initialize_once_when_database_is_available() {
        if std::env::var("TEST_DATABASE_URL").is_err() {
            return;
        }

        let _first = test_pool().await.expect("test pool");
        let before = TEST_MIGRATION_INITIALIZATIONS.load(std::sync::atomic::Ordering::Relaxed);
        let _second = test_pool().await.expect("test pool reuse");

        assert_eq!(
            TEST_MIGRATION_INITIALIZATIONS.load(std::sync::atomic::Ordering::Relaxed),
            before
        );
    }
}
