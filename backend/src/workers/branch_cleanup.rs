use crate::repositories::devrail_runs;
use reqwest::Client;
use sqlx::PgPool;
use std::time::Duration;

async fn delete_remote_branch(
    client: &Client,
    remote_url: Option<&str>,
    credential_ref: Option<&str>,
    branch: &str,
) -> Result<bool, String> {
    let remote_url = remote_url.ok_or_else(|| "仓库远端地址不存在".to_string())?;
    let (provider, host) = if remote_url.contains("github.com") {
        ("github", "github.com")
    } else if remote_url.contains("gitlab.com") {
        ("gitlab", "gitlab.com")
    } else {
        return Err("仓库平台不支持远程分支清理".to_string());
    };
    let tail = remote_url
        .split(host)
        .nth(1)
        .unwrap_or_default()
        .trim_matches(&['/', ':'][..])
        .trim_end_matches(".git");
    let mut parts = tail.rsplitn(2, '/');
    let repository = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    if owner.is_empty() || repository.is_empty() {
        return Err("仓库远端地址格式无效".to_string());
    }
    let env_name = credential_ref.unwrap_or(if provider == "github" {
        "DEVRAIL_GITHUB_TOKEN"
    } else {
        "DEVRAIL_GITLAB_TOKEN"
    });
    let token = std::env::var(env_name).map_err(|_| "Git 平台凭据未配置".to_string())?;
    let response = if provider == "github" {
        client
            .delete(format!(
                "https://api.github.com/repos/{owner}/{repository}/git/refs/heads/{branch}"
            ))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| "删除 GitHub 远程分支失败".to_string())?
    } else {
        let project_path = format!("{owner}/{repository}");
        let project = urlencoding::encode(&project_path);
        let branch = urlencoding::encode(branch);
        client
            .delete(format!(
                "https://gitlab.com/api/v4/projects/{project}/repository/branches/{branch}"
            ))
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|_| "删除 GitLab 远程分支失败".to_string())?
    };
    Ok(response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND)
}

pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let client = match Client::builder().user_agent("DevRail/1.0").build() {
                Ok(client) => client,
                Err(error) => {
                    tracing::error!(error = %error, "branch cleanup client initialization failed");
                    continue;
                }
            };
            match devrail_runs::expired_branches(&pool).await {
                Ok(rows) => {
                    for row in rows {
                        match delete_remote_branch(
                            &client,
                            row.remote_url.as_deref(),
                            row.credential_ref.as_deref(),
                            &row.branch_name,
                        )
                        .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                tracing::error!(run_id = row.run_id, "远程临时分支清理被拒绝");
                                continue;
                            }
                            Err(error) => {
                                tracing::error!(run_id = row.run_id, error = %error, "远程临时分支清理失败");
                                continue;
                            }
                        }
                        let mut tx = match pool.begin().await {
                            Ok(tx) => tx,
                            Err(error) => {
                                tracing::error!(error = %error, "branch cleanup transaction failed");
                                continue;
                            }
                        };
                        if let Err(error) =
                            devrail_runs::clear_expired_branch(&mut tx, row.run_id).await
                        {
                            tracing::error!(run_id = row.run_id, error = %error, "branch expiry cleanup failed");
                            continue;
                        }
                        if let Err(error) = tx.commit().await {
                            tracing::error!(run_id = row.run_id, error = %error, "branch expiry cleanup commit failed");
                        } else {
                            tracing::info!(run_id = row.run_id, branch = %row.branch_name, "expired temporary branch removed and binding cleared");
                        }
                    }
                }
                Err(error) => tracing::error!(error = %error, "branch cleanup worker failed"),
            }
        }
    });
}
