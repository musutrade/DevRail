use crate::config::{ExternalValuePolicy, ServiceConfig};
use crate::project::Project;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::{Host, Url};

const ALLOW_REMOTE_TEST_DATABASE_ENV: &str = "ARC_FLOW_ALLOW_REMOTE_TEST_DATABASE";

pub struct ServiceManager<'a> {
    project: &'a Project,
    running: BTreeMap<String, RunningService>,
    failures: BTreeMap<String, String>,
}

impl<'a> ServiceManager<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self {
            project,
            running: BTreeMap::new(),
            failures: BTreeMap::new(),
        }
    }

    pub fn environment(&mut self, id: &str) -> Result<(String, String)> {
        if let Some(error) = self.failures.get(id) {
            bail!("service {id:?} previously failed: {error}");
        }
        if !self.running.contains_key(id) {
            let config = self
                .project
                .config
                .service(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown service {id:?}"))?;
            let running = match RunningService::start(self.project, id, config) {
                Ok(running) => running,
                Err(error) => {
                    let detail = format!("{error:#}");
                    self.failures.insert(id.to_string(), detail.clone());
                    bail!("{detail}");
                }
            };
            self.running.insert(id.to_string(), running);
        }
        let service = self.running.get(id).expect("service inserted");
        Ok((service.inject_env.clone(), service.value.clone()))
    }
}

struct RunningService {
    inject_env: String,
    value: String,
    container: Option<String>,
    project_root: PathBuf,
}

struct DockerStartOptions {
    image: String,
    inject_env: String,
    startup_timeout_secs: u64,
    container_port: u16,
    environment: BTreeMap<String, String>,
    healthcheck: Vec<String>,
    connection: String,
    deadline: Instant,
}

impl RunningService {
    fn start(project: &Project, id: &str, config: ServiceConfig) -> Result<Self> {
        match config {
            ServiceConfig::Environment {
                source_env,
                inject_env,
            } => {
                let value = std::env::var(&source_env)
                    .with_context(|| format!("service {id:?} requires {source_env}"))?;
                if value.trim().is_empty() {
                    bail!("service {id:?} requires non-empty {source_env}");
                }
                Ok(Self {
                    inject_env,
                    value,
                    container: None,
                    project_root: project.root.clone(),
                })
            }
            ServiceConfig::Docker {
                image,
                external_env,
                inject_env,
                external_value_policy,
                startup_timeout_secs,
                container_port,
                environment,
                healthcheck,
                connection,
                ..
            } => {
                if let Some((_, value)) = external_env
                    .as_ref()
                    .and_then(|name| std::env::var(name).ok().map(|value| (name, value)))
                    .filter(|(_, value)| !value.trim().is_empty())
                {
                    validate_external_value(external_value_policy, &value)?;
                    return Ok(Self {
                        inject_env,
                        value,
                        container: None,
                        project_root: project.root.clone(),
                    });
                }
                let deadline = Instant::now() + Duration::from_secs(startup_timeout_secs);
                ensure_docker(project, id, remaining(deadline)?)?;
                start_docker(
                    project,
                    id,
                    DockerStartOptions {
                        image,
                        inject_env,
                        startup_timeout_secs,
                        container_port,
                        environment,
                        healthcheck,
                        connection,
                        deadline,
                    },
                )
            }
        }
    }
}

fn validate_external_value(policy: ExternalValuePolicy, value: &str) -> Result<()> {
    match policy {
        ExternalValuePolicy::None => Ok(()),
        ExternalValuePolicy::IsolatedPostgres => {
            let production_url = std::env::var("DATABASE_URL").ok();
            let allow_remote = std::env::var(ALLOW_REMOTE_TEST_DATABASE_ENV)
                .is_ok_and(|flag| matches!(flag.as_str(), "1" | "true"));
            validate_isolated_postgres_url(value, production_url.as_deref(), allow_remote)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PostgresTarget {
    host: String,
    port: u16,
    database: String,
    loopback: bool,
}

fn parse_postgres_target(value: &str) -> Result<PostgresTarget> {
    let url = Url::parse(value).context("test database URL must be a valid URL")?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        bail!("test database URL must use the postgres or postgresql scheme");
    }
    let (host, loopback) = match url
        .host()
        .context("test database URL must include a host")?
    {
        Host::Domain(host) => {
            let normalized = host.to_ascii_lowercase();
            let loopback = host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
            (normalized, loopback)
        }
        Host::Ipv4(address) => (address.to_string(), IpAddr::V4(address).is_loopback()),
        Host::Ipv6(address) => (address.to_string(), IpAddr::V6(address).is_loopback()),
    };
    let mut segments = url
        .path_segments()
        .context("test database URL must include a database name")?;
    let database = segments
        .next()
        .filter(|segment| !segment.is_empty())
        .context("test database URL must include a database name")?
        .to_ascii_lowercase();
    if segments.any(|segment| !segment.is_empty()) {
        bail!("test database URL must contain exactly one database name");
    }
    Ok(PostgresTarget {
        host,
        port: url.port().unwrap_or(5432),
        database,
        loopback,
    })
}

fn validate_isolated_postgres_url(
    value: &str,
    production_url: Option<&str>,
    allow_remote: bool,
) -> Result<()> {
    let target = parse_postgres_target(value)?;
    if !target.database.ends_with("_test") && !target.database.ends_with("-test") {
        bail!("test database name must end with _test or -test");
    }
    if !target.loopback && !allow_remote {
        bail!(
            "remote test databases require {ALLOW_REMOTE_TEST_DATABASE_ENV}=1 after isolation is verified"
        );
    }
    if let Some(production_url) = production_url {
        if value == production_url {
            bail!("TEST_DATABASE_URL must not equal DATABASE_URL");
        }
        if let Ok(production) = parse_postgres_target(production_url) {
            let same_host =
                target.host == production.host || (target.loopback && production.loopback);
            if same_host && target.port == production.port && target.database == production.database
            {
                bail!("TEST_DATABASE_URL must not target the DATABASE_URL database");
            }
        }
    }
    Ok(())
}

fn start_docker(
    project: &Project,
    id: &str,
    options: DockerStartOptions,
) -> Result<RunningService> {
    let DockerStartOptions {
        image,
        inject_env,
        startup_timeout_secs,
        container_port,
        environment,
        healthcheck,
        connection,
        deadline,
    } = options;
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = format!(
        "arc-flow-{}-{id}-{}-{unique}",
        project.config.project.name,
        std::process::id()
    );
    let publish = format!("127.0.0.1::{container_port}");
    let mut args = vec![
        "run".to_string(),
        "--rm".into(),
        "--detach".into(),
        "--pull=never".into(),
        "--name".into(),
        name.clone(),
    ];
    for (key, value) in environment {
        args.extend(["--env".into(), format!("{key}={value}")]);
    }
    args.extend(["--publish".into(), publish, image.clone()]);
    let output = crate::process::capture("docker", &args, &project.root, remaining(deadline)?)
        .with_context(|| format!("start Docker service {id:?}"))?;
    if !output.status.success() {
        bail!(
            "failed to start Docker service {id:?} with image {image}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let mut running = RunningService {
        inject_env,
        value: String::new(),
        container: Some(name),
        project_root: project.root.clone(),
    };
    while Instant::now() < deadline {
        if crate::process::cancelled() {
            bail!("verification cancelled while waiting for service {id:?}");
        }
        if let Some(port) = running.port(container_port, remaining(deadline)?)? {
            let mut health_args = vec![
                "exec".to_string(),
                running.container.as_deref().unwrap_or_default().to_string(),
            ];
            health_args.extend(healthcheck.iter().cloned());
            let ready = crate::process::capture(
                "docker",
                &health_args,
                &project.root,
                remaining(deadline)?,
            )?
            .status
            .success();
            if ready {
                running.value = connection.replace("{host_port}", &port);
                return Ok(running);
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
    bail!("service {id:?} did not become ready within {startup_timeout_secs} seconds")
}

fn ensure_docker(project: &Project, id: &str, timeout: Duration) -> Result<()> {
    let info = crate::process::capture("docker", &["info".to_string()], &project.root, timeout)
        .with_context(|| format!("Docker is required by service {id:?}"))?;
    if !info.status.success() {
        bail!("Docker daemon is unavailable for service {id:?}");
    }
    Ok(())
}

impl RunningService {
    fn port(&self, container_port: u16, timeout: Duration) -> Result<Option<String>> {
        let Some(container) = &self.container else {
            return Ok(None);
        };
        let args = vec![
            "port".into(),
            container.clone(),
            format!("{container_port}/tcp"),
        ];
        let output = crate::process::capture("docker", &args, &self.project_root, timeout)?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .and_then(|line| line.rsplit(':').next())
            .map(str::to_string))
    }
}

impl Drop for RunningService {
    fn drop(&mut self) {
        if let Some(container) = &self.container {
            let args = vec!["rm".into(), "--force".into(), container.clone()];
            let _ = crate::process::capture_cleanup(
                "docker",
                &args,
                &self.project_root,
                Duration::from_secs(5),
            );
        }
    }
}

pub fn check_available(project: &Project, id: &str, timeout: Duration) -> Result<String> {
    let service = project
        .config
        .service(id)
        .ok_or_else(|| anyhow::anyhow!("unknown service {id:?}"))?;
    match service {
        ServiceConfig::Environment { source_env, .. } => {
            let value = std::env::var(source_env)
                .with_context(|| format!("{source_env} is not configured"))?;
            if value.trim().is_empty() {
                bail!("{source_env} is empty");
            }
            Ok(format!("{source_env} is configured"))
        }
        ServiceConfig::Docker {
            image,
            external_env,
            external_value_policy,
            ..
        } => {
            if let Some(name) = external_env {
                if let Ok(value) = std::env::var(name) {
                    if !value.trim().is_empty() {
                        validate_external_value(*external_value_policy, &value)?;
                        return Ok(format!("{name} is configured"));
                    }
                }
            }
            let deadline = Instant::now() + timeout;
            ensure_docker(project, id, remaining(deadline)?)?;
            let args = vec!["image".into(), "inspect".into(), image.clone()];
            let image_ready =
                crate::process::capture("docker", &args, &project.root, remaining(deadline)?)?
                    .status
                    .success();
            if !image_ready {
                bail!("Docker image {image} is not available; run `docker pull {image}`");
            }
            Ok(format!("Docker and {image} ready"))
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| anyhow::anyhow!("service operation timed out"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_isolated_loopback_database() {
        let result = validate_isolated_postgres_url(
            "postgres://tester:secret@127.0.0.1:5432/arc_admin_test",
            Some("postgres://developer:secret@localhost:5432/arc_admin"),
            false,
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn accepts_an_ipv6_loopback_database() {
        assert!(validate_isolated_postgres_url(
            "postgres://tester:secret@[::1]:5432/arc_admin_test",
            None,
            false,
        )
        .is_ok());
    }

    #[test]
    fn rejects_a_database_without_a_test_suffix() {
        let error = validate_isolated_postgres_url(
            "postgres://tester:secret@localhost/arc_admin",
            None,
            false,
        )
        .expect_err("production-like database name must fail");
        assert!(error.to_string().contains("must end with"));
    }

    #[test]
    fn rejects_a_remote_database_without_an_explicit_override() {
        let error = validate_isolated_postgres_url(
            "postgresql://tester:secret@test-db.example.com/arc_admin_test",
            None,
            false,
        )
        .expect_err("remote database must fail closed");
        assert!(error.to_string().contains(ALLOW_REMOTE_TEST_DATABASE_ENV));
    }

    #[test]
    fn accepts_an_explicitly_allowed_remote_test_database() {
        assert!(validate_isolated_postgres_url(
            "postgres://tester:secret@test-db.example.com/arc-admin-test",
            None,
            true,
        )
        .is_ok());
    }

    #[test]
    fn rejects_the_configured_runtime_database() {
        let error = validate_isolated_postgres_url(
            "postgres://tester:secret@127.0.0.1:5432/arc_admin_test",
            Some("postgresql://runtime:secret@localhost:5432/arc_admin_test"),
            false,
        )
        .expect_err("same database target must fail");
        assert!(
            error.to_string().contains("DATABASE_URL database"),
            "{error:?}"
        );
    }
}
