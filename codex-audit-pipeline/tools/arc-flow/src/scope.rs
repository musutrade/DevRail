use crate::config::UnmatchedScope;
use crate::project::Project;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ScopeMode {
    WorkingTree,
    Staged,
    Base(String),
    All,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeResult {
    pub mode: String,
    pub changed_files: Vec<String>,
    pub components: BTreeSet<String>,
    pub unmatched_files: Vec<String>,
}

impl ScopeResult {
    pub fn all(project: &Project) -> Self {
        Self {
            mode: "all".to_string(),
            changed_files: Vec::new(),
            components: project.config.components(),
            unmatched_files: Vec::new(),
        }
    }

    pub fn write_reports(&self, project: &Project) -> Result<()> {
        std::fs::create_dir_all(&project.reports)?;
        let changed = if self.changed_files.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.changed_files.join("\n"))
        };
        std::fs::write(project.reports.join("changed_files.txt"), changed)?;
        std::fs::write(
            project.reports.join("scope.json"),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }
}

pub fn detect(project: &Project, mode: &ScopeMode) -> Result<ScopeResult> {
    if matches!(mode, ScopeMode::All) {
        return Ok(ScopeResult::all(project));
    }

    ensure_git_worktree(project)?;
    let mut paths = BTreeSet::new();
    let mode_label = match mode {
        ScopeMode::WorkingTree => {
            paths.extend(git_paths(project, &["diff", "--name-only", "-z"])?);
            paths.extend(git_paths(
                project,
                &["diff", "--cached", "--name-only", "-z"],
            )?);
            paths.extend(git_paths(
                project,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )?);
            "working-tree".to_string()
        }
        ScopeMode::Staged => {
            paths.extend(git_paths(
                project,
                &["diff", "--cached", "--name-only", "-z"],
            )?);
            "staged".to_string()
        }
        ScopeMode::Base(reference) => {
            let output = git_capture(
                project,
                vec![
                    "rev-parse".into(),
                    "--verify".into(),
                    format!("{reference}^{{commit}}"),
                ],
            )
            .context("run git rev-parse")?;
            if !output.status.success() {
                bail!("Git base reference does not exist: {reference}");
            }
            paths.extend(git_paths(
                project,
                &["diff", "--name-only", "-z", &format!("{reference}...HEAD")],
            )?);
            format!("base:{reference}")
        }
        ScopeMode::All => unreachable!(),
    };

    let changed_files = paths.into_iter().collect::<Vec<_>>();
    let (mut components, unmatched_files) = project.config.classify_paths(&changed_files)?;
    match project.config.scope.unmatched {
        UnmatchedScope::Fail if !unmatched_files.is_empty() => {
            bail!(
                "scope has {} unmatched changed file(s): {}",
                unmatched_files.len(),
                unmatched_files.join(", ")
            );
        }
        UnmatchedScope::All => components.extend(project.config.components()),
        UnmatchedScope::Fail | UnmatchedScope::Ignore => {}
    }
    Ok(ScopeResult {
        mode: mode_label,
        components,
        changed_files,
        unmatched_files,
    })
}

fn ensure_git_worktree(project: &Project) -> Result<()> {
    let output = git_capture(
        project,
        vec!["rev-parse".into(), "--is-inside-work-tree".into()],
    )
    .context("run git rev-parse")?;
    if !output.status.success() || output.stdout != b"true\n" {
        bail!("project root is not a Git worktree");
    }
    Ok(())
}

fn git_paths(project: &Project, args: &[&str]) -> Result<Vec<String>> {
    let output = git_capture(project, args.iter().map(|arg| (*arg).to_string()).collect())
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            String::from_utf8(entry.to_vec()).context("Git returned a non-UTF-8 file path")
        })
        .collect()
}

fn git_capture(project: &Project, args: Vec<String>) -> Result<crate::process::CapturedOutput> {
    crate::process::capture("git", &args, &project.root, Duration::from_secs(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> crate::config::FlowConfig {
        toml::from_str(include_str!("../../../../.arc-flow/flow.toml")).expect("parse config")
    }

    #[test]
    fn workflow_changes_force_all_components() {
        let components = config()
            .classify_paths(&["codex-audit-pipeline/tools/arc-flow/src/main.rs".into()])
            .expect("classify")
            .0;
        assert_eq!(components.len(), 3);
    }

    #[test]
    fn frontend_change_only_selects_frontend() {
        let components = config()
            .classify_paths(&["frontend/src/main.ts".into()])
            .expect("classify")
            .0;
        assert_eq!(components, BTreeSet::from(["frontend".to_string()]));
    }

    #[test]
    fn unmatched_paths_are_reported() {
        let (components, unmatched) = config()
            .classify_paths(&["unconfigured/new-tool.lock".into()])
            .expect("classify");

        assert!(components.is_empty());
        assert_eq!(unmatched, vec!["unconfigured/new-tool.lock"]);
    }
}
