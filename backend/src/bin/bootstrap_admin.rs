use arc_admin_backend::{db, services};
use std::env;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if env_path.is_file() {
        dotenvy::from_path(&env_path)?;
    }

    let database_url = env::var("DATABASE_URL")?;
    let username = env::var("BOOTSTRAP_ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let password = env::var("BOOTSTRAP_ADMIN_PASSWORD")
        .map_err(|_| anyhow::anyhow!("必须设置 BOOTSTRAP_ADMIN_PASSWORD"))?;
    let display_name =
        env::var("BOOTSTRAP_ADMIN_DISPLAY_NAME").unwrap_or_else(|_| "管理员".to_string());
    let email = env::var("BOOTSTRAP_ADMIN_EMAIL").ok();

    let pool = db::init_pool(&database_url).await?;
    db::run_migrations(&pool).await?;
    let user =
        services::auth::bootstrap_super_admin(&pool, &username, &password, &display_name, email)
            .await?;
    println!("管理员账号已就绪：{}（id={}）", user.username, user.id);
    Ok(())
}
