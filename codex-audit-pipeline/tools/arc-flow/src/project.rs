use crate::config::{resolve_config_path, FlowConfig, DEFAULT_CONFIG_PATH};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config_path: PathBuf,
    pub config: FlowConfig,
    pub reports: PathBuf,
    pub audit_config: PathBuf,
    pub secrets_config: PathBuf,
    aliases: BTreeMap<String, PathBuf>,
}

impl Project {
    pub fn discover(
        override_root: Option<PathBuf>,
        config_override: Option<PathBuf>,
    ) -> Result<Self> {
        let root = match override_root.or_else(|| env::var_os("PROJECT_ROOT").map(PathBuf::from)) {
            Some(root) => canonical_directory(&root)?,
            None => {
                let start = env::current_dir().context("read current directory")?;
                find_root(&start, config_override.as_deref())?
            }
        };
        let config_path = resolve_config_path(&root, config_override)?;
        let config = FlowConfig::load(&config_path)?;
        let reports = resolve_repo_path(
            &root,
            Path::new(&config.paths.reports),
            "report directory",
            false,
        )?;
        let audit_config = resolve_repo_path(
            &root,
            Path::new(&config.paths.audit_config),
            "audit configuration",
            true,
        )?;
        let secrets_config = resolve_repo_path(
            &root,
            Path::new(&config.paths.secrets_config),
            "secret scan configuration",
            true,
        )?;
        let aliases = config
            .paths
            .aliases
            .iter()
            .map(|(name, entry)| {
                resolve_repo_path(
                    &root,
                    Path::new(&entry.path),
                    &format!("path alias {name:?}"),
                    false,
                )
                .map(|path| (name.clone(), path))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;

        let project = Self {
            root,
            config_path,
            config,
            reports,
            audit_config,
            secrets_config,
            aliases,
        };
        project.validate()?;
        Ok(project)
    }

    fn validate(&self) -> Result<()> {
        if !self.audit_config.is_file() {
            bail!(
                "required audit configuration is missing: {}",
                self.audit_config.display()
            );
        }
        if !self.secrets_config.is_file() {
            bail!(
                "required secret scan configuration is missing: {}",
                self.secrets_config.display()
            );
        }
        Ok(())
    }

    pub fn prepare(&self) -> Result<()> {
        let reports = resolve_repo_path(
            &self.root,
            Path::new(&self.config.paths.reports),
            "report directory",
            false,
        )?;
        if reports != self.reports {
            bail!("report directory changed during project discovery");
        }
        fs::create_dir_all(self.reports.join("logs"))?;
        env::set_current_dir(&self.root)
            .with_context(|| format!("enter project root {}", self.root.display()))?;
        Ok(())
    }

    pub fn path(&self, alias: &str) -> Option<&Path> {
        match alias {
            "root" => Some(&self.root),
            "reports" => Some(&self.reports),
            "audit_config" => Some(&self.audit_config),
            "secrets_config" => Some(&self.secrets_config),
            _ => self.aliases.get(alias).map(PathBuf::as_path),
        }
    }

    pub fn expand(&self, value: &str) -> String {
        let mut resolved = value.to_string();
        for name in self.config.paths.aliases.keys().map(String::as_str) {
            if let Some(path) = self.path(name) {
                resolved = resolved.replace(&format!("{{{name}}}"), &path.to_string_lossy());
            }
        }
        for name in ["audit_config", "secrets_config", "reports", "root"] {
            if let Some(path) = self.path(name) {
                resolved = resolved.replace(&format!("{{{name}}}"), &path.to_string_lossy());
            }
        }
        resolved
    }
}

pub(crate) fn resolve_repo_path(
    root: &Path,
    path: &Path,
    label: &str,
    must_exist: bool,
) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolve repository root {}", root.display()))?;
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("{label} must be a non-empty repository-relative path");
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!("{label} may not escape the repository: {}", path.display());
    }

    let candidate = root.join(path);
    if fs::symlink_metadata(&candidate).is_ok() {
        let resolved = candidate
            .canonicalize()
            .with_context(|| format!("resolve {label} {}", candidate.display()))?;
        if !resolved.starts_with(root) {
            bail!("{label} escapes the repository: {}", candidate.display());
        }
        return Ok(resolved);
    }
    if must_exist {
        bail!("{label} is missing: {}", candidate.display());
    }

    let mut ancestor = candidate.as_path();
    while fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve {label}: {}", candidate.display()))?;
    }
    let resolved_ancestor = ancestor
        .canonicalize()
        .with_context(|| format!("resolve {label} parent {}", ancestor.display()))?;
    if !resolved_ancestor.starts_with(root) {
        bail!("{label} escapes the repository: {}", candidate.display());
    }
    Ok(candidate)
}

fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve project path {}", path.display()))?;
    if !path.is_dir() {
        bail!("project root is not a directory: {}", path.display());
    }
    Ok(path)
}

fn find_root(start: &Path, config_override: Option<&Path>) -> Result<PathBuf> {
    let start = canonical_directory(start)?;
    let configured_path = config_override
        .map(Path::to_path_buf)
        .or_else(|| env::var_os("ARC_FLOW_CONFIG").map(PathBuf::from));
    if let Some(config) = configured_path {
        let candidate = if config.is_absolute() {
            config
        } else {
            start.join(config)
        };
        let config = candidate
            .canonicalize()
            .with_context(|| format!("resolve workflow config {}", candidate.display()))?;
        if let Some(root) = config.ancestors().find(|path| path.join(".git").exists()) {
            return Ok(root.to_path_buf());
        }
    }
    start
        .ancestors()
        .find(|candidate| candidate.join(DEFAULT_CONFIG_PATH).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "could not find project root above {}; expected {}; run `arc-flow init --preset <name>`",
                start.display(),
                DEFAULT_CONFIG_PATH
            )
        })
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    fn fixture(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "arc-flow-project-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create fixture");
        path
    }

    #[cfg(unix)]
    #[test]
    fn repository_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = fixture("root");
        let outside = fixture("outside");
        symlink(&outside, root.join("reports")).expect("create symlink");

        let error = resolve_repo_path(&root, Path::new("reports"), "reports", false)
            .expect_err("symlink escape must fail");

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
        assert!(error.to_string().contains("escapes the repository"));
    }
}
