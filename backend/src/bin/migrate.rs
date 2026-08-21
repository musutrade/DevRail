use arc_admin_backend::db::{self, DatabasePoolConfig};
use std::env;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if env_path.is_file() {
        dotenvy::from_path(&env_path)?;
    }

    let database_url = env::var("DATABASE_URL")?;
    let pool_config = DatabasePoolConfig::from_env()?;
    let pool = db::init_pool_with_config(&database_url, &pool_config).await?;
    let status = db::run_migrations(&pool).await?;
    pool.close().await;
    println!(
        "数据库迁移完成：已应用 {}，内置 {}",
        status.applied, status.embedded
    );
    Ok(())
}
