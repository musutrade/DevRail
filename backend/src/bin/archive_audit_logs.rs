use arc_admin_backend::db::{self, DatabasePoolConfig};
use arc_admin_backend::models::AuditArchiveRow;
use arc_admin_backend::repositories::audit_logs;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveManifest {
    schema_version: u8,
    archive_file: String,
    sha256: String,
    record_count: usize,
    first_id: i64,
    last_id: i64,
    cutoff: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if env_path.is_file() {
        dotenvy::from_path(&env_path)?;
    }

    let database_url = env::var("DATABASE_URL")?;
    let retention_days = positive_i64("AUDIT_RETENTION_DAYS", 365)?;
    let batch_size = positive_i64("AUDIT_ARCHIVE_BATCH_SIZE", 10_000)?.min(100_000);
    let output_dir = PathBuf::from(
        env::var("AUDIT_ARCHIVE_DIR").unwrap_or_else(|_| "audit-archives".to_string()),
    );
    fs::create_dir_all(&output_dir)?;

    let cutoff = Utc::now() - Duration::days(retention_days);
    let pool_config = DatabasePoolConfig::from_env()?;
    let pool = db::init_pool_with_config(&database_url, &pool_config).await?;
    let mut archived = 0_u64;

    loop {
        let rows = audit_logs::archive_batch(&pool, cutoff, batch_size).await?;
        if rows.is_empty() {
            break;
        }
        let ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
        let archive_path = write_archive(&output_dir, cutoff, &rows)?;
        let deleted = audit_logs::delete_archived(&pool, &ids, cutoff).await?;
        archived += deleted;
        println!(
            "审计归档完成：{} 条，文件 {}",
            deleted,
            archive_path.display()
        );
    }

    pool.close().await;
    println!("审计保留任务完成：归档并删除 {archived} 条过期记录");
    Ok(())
}

fn positive_i64(name: &str, default: i64) -> anyhow::Result<i64> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| anyhow::anyhow!("{name} 必须是正整数"))?
        .unwrap_or(default);
    if value <= 0 {
        anyhow::bail!("{name} 必须是正整数");
    }
    Ok(value)
}

fn write_archive(
    output_dir: &Path,
    cutoff: DateTime<Utc>,
    rows: &[AuditArchiveRow],
) -> anyhow::Result<PathBuf> {
    let first = rows
        .first()
        .ok_or_else(|| anyhow::anyhow!("归档批次为空"))?;
    let last = rows.last().ok_or_else(|| anyhow::anyhow!("归档批次为空"))?;
    let stem = format!(
        "audit-{}-{}-{}",
        first.id,
        last.id,
        cutoff.format("%Y%m%dT%H%M%SZ")
    );
    let archive_path = output_dir.join(format!("{stem}.jsonl"));
    let archive_temp = output_dir.join(format!(".{stem}.jsonl.{}.tmp", std::process::id()));
    let mut file = create_new(&archive_temp)?;
    let mut hasher = Sha256::new();
    {
        let mut writer = BufWriter::new(&mut file);
        for row in rows {
            let mut line = serde_json::to_vec(row)?;
            line.push(b'\n');
            hasher.update(&line);
            writer.write_all(&line)?;
        }
        writer.flush()?;
    }
    file.sync_all()?;
    fs::rename(&archive_temp, &archive_path)?;

    let manifest = ArchiveManifest {
        schema_version: 1,
        archive_file: archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("归档文件名不是有效 UTF-8"))?
            .to_string(),
        sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        record_count: rows.len(),
        first_id: first.id,
        last_id: last.id,
        cutoff,
        created_at: Utc::now(),
    };
    let manifest_path = output_dir.join(format!("{stem}.manifest.json"));
    let manifest_temp =
        output_dir.join(format!(".{stem}.manifest.json.{}.tmp", std::process::id()));
    let mut manifest_file = create_new(&manifest_temp)?;
    serde_json::to_writer_pretty(&mut manifest_file, &manifest)?;
    manifest_file.write_all(b"\n")?;
    manifest_file.sync_all()?;
    fs::rename(&manifest_temp, manifest_path)?;
    File::open(output_dir)?.sync_all()?;
    Ok(archive_path)
}

fn create_new(path: &Path) -> anyhow::Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}
