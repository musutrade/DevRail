use crate::audit;
use crate::config::{ParserConfig, StepConfig, TestIsolationMode};
use crate::process::{Task, TaskResult};
use crate::project::Project;
use crate::scope::ScopeResult;
use crate::secrets::{self, SecretMode};
use crate::service::ServiceManager;
use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub timestamp: String,
    pub profile: String,
    pub scope: ScopeResult,
    pub steps: Vec<TaskResult>,
    pub passed: bool,
}

impl VerificationReport {
    fn write(&self, project: &Project) -> Result<()> {
        fs::create_dir_all(&project.reports)?;
        fs::write(
            project.reports.join("test_result.json"),
            serde_json::to_string_pretty(self)?,
        )?;

        let mut markdown = String::from("=== Verification report ===\n");
        markdown.push_str(&format!("Timestamp: {}\n", self.timestamp));
        markdown.push_str(&format!("Profile: {}\n", self.profile));
        markdown.push_str(&format!(
            "Components: {}\n\n",
            self.scope
                .components
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ));
        for step in &self.steps {
            let status = if step.passed { "PASS" } else { "FAIL" };
            markdown.push_str(&format!(
                "- {status}: {} ({} ms)",
                step.label, step.duration_ms
            ));
            if let Some(detail) = &step.detail {
                markdown.push_str(&format!(" - {detail}"));
            }
            if !step.passed {
                markdown.push_str(&format!("; log: {}", step.log));
            }
            markdown.push('\n');
        }
        markdown.push_str(&format!(
            "\nTEST_SUMMARY: {}\n",
            if self.passed { "PASS" } else { "FAIL" }
        ));
        fs::write(project.reports.join("test_result.md"), markdown)?;
        Ok(())
    }
}

pub fn run(
    project: &Project,
    scope: ScopeResult,
    profile: &str,
    staged: bool,
) -> Result<VerificationReport> {
    if !project
        .config
        .steps
        .iter()
        .any(|step| step.profiles.contains(profile))
    {
        bail!("unknown or empty verification profile {profile:?}");
    }
    run_selected(project, scope, profile, staged, None)
}

pub fn run_step(project: &Project, id: &str) -> Result<VerificationReport> {
    let step = project
        .config
        .step(id)
        .ok_or_else(|| anyhow::anyhow!("unknown verification step {id:?}"))?;
    let scope = explicit_scope(std::slice::from_ref(&step.component));
    run_selected(
        project,
        scope,
        &project.config.project.default_profile,
        false,
        Some(id),
    )
}

fn run_selected(
    project: &Project,
    scope: ScopeResult,
    profile: &str,
    staged: bool,
    only_step: Option<&str>,
) -> Result<VerificationReport> {
    println!("arc-flow verify");
    println!("Scope: {}", scope.mode);
    println!(
        "Components: {}\n",
        if scope.components.is_empty() {
            "none".to_string()
        } else {
            scope
                .components
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    scope.write_reports(project)?;
    let mut steps = Vec::new();
    let secret_mode = if staged {
        SecretMode::Staged
    } else {
        SecretMode::WorkingTree
    };
    let secret_started = Instant::now();
    let findings = secrets::scan(project, secret_mode)?;
    let secret_passed = findings.is_empty();
    let secret_result = TaskResult {
        label: "secret scan".to_string(),
        passed: secret_passed,
        timed_out: false,
        cancelled: false,
        duration_ms: secret_started.elapsed().as_millis(),
        log: project
            .reports
            .join("secret_scan.json")
            .to_string_lossy()
            .to_string(),
        detail: (!secret_passed).then(|| format!("{} file(s) require review", findings.len())),
    };
    print_result(&secret_result);
    steps.push(secret_result);

    if secret_passed {
        let audit_started = Instant::now();
        let outcome = audit::run(
            &project.root,
            &project.audit_config,
            &project.reports,
            false,
        )?;
        let audit_passed = outcome.total_violations == 0;
        let audit_result = TaskResult {
            label: "architecture audit".to_string(),
            passed: audit_passed,
            timed_out: false,
            cancelled: false,
            duration_ms: audit_started.elapsed().as_millis(),
            log: outcome.report_file.to_string_lossy().to_string(),
            detail: Some(format!(
                "{} violation(s), {} blocker(s), {} error(s), {} warning(s)",
                outcome.total_violations,
                outcome.blocker_count,
                outcome.error_count,
                outcome.warning_count
            )),
        };
        print_result(&audit_result);
        steps.push(audit_result);
    }

    if crate::process::cancelled() {
        bail!("verification cancelled");
    }
    if steps.iter().all(|step| step.passed) {
        run_configured_steps(project, &scope, profile, only_step, &mut steps)?;
    }

    let passed = steps.iter().all(|step| step.passed);
    let report = VerificationReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        profile: only_step
            .map(|id| format!("step:{id}"))
            .unwrap_or_else(|| profile.to_string()),
        scope,
        steps,
        passed,
    };
    report.write(project)?;
    println!(
        "\nVerification report: {}",
        project.reports.join("test_result.md").display()
    );
    println!(
        "TEST_SUMMARY: {}",
        if report.passed { "PASS" } else { "FAIL" }
    );
    Ok(report)
}

fn run_configured_steps(
    project: &Project,
    scope: &ScopeResult,
    profile: &str,
    only_step: Option<&str>,
    results: &mut Vec<TaskResult>,
) -> Result<()> {
    let selected = project.config.steps.iter().filter(|step| {
        scope.components.contains(&step.component)
            && only_step
                .map(|id| step.id == id)
                .unwrap_or_else(|| step.profiles.contains(profile))
    });
    let mut services = ServiceManager::new(project);

    'steps: for step in selected {
        if crate::process::cancelled() {
            bail!("verification cancelled");
        }
        let mut service_env = Vec::new();
        for service in &step.services {
            let environment = match services.environment(service) {
                Ok(environment) => environment,
                Err(error) => {
                    let result = TaskResult {
                        label: format!("{}: service {service} setup", step.label),
                        passed: false,
                        timed_out: false,
                        cancelled: false,
                        duration_ms: 0,
                        log: String::new(),
                        detail: Some(format!("{error:#}")),
                    };
                    print_result(&result);
                    results.push(result);
                    continue 'steps;
                }
            };
            service_env.push(environment);
        }
        let parser = step
            .parser
            .as_deref()
            .and_then(|id| project.config.parser(id));
        execute(configured_task(project, step, service_env), parser, results)?;
    }
    Ok(())
}

fn configured_task(
    project: &Project,
    step: &StepConfig,
    service_env: Vec<(String, String)>,
) -> Task {
    let cwd = std::path::PathBuf::from(project.expand(&step.cwd));
    let mut args = step
        .args
        .iter()
        .map(|argument| project.expand(argument))
        .collect::<Vec<_>>();
    let label = step
        .test_threads
        .map(|threads| {
            let isolation = match step.test_isolation {
                Some(TestIsolationMode::Schema) => ", isolation: schema",
                Some(TestIsolationMode::Shared) => ", isolation: shared",
                None => "",
            };
            format!("{} (threads: {threads}{isolation})", step.label)
        })
        .unwrap_or_else(|| step.label.clone());
    if let Some(test_threads) = step.test_threads {
        args.push("--test-threads".into());
        args.push(test_threads.to_string());
    }
    let mut task = Task::new(label, &step.program, &cwd, log(project, &step.log))
        .args(args)
        .timeout(step.timeout_secs);
    if let Some(test_threads) = step.test_threads {
        task = task.env("DEVRAIL_TEST_THREADS", test_threads.to_string());
    }
    for (name, value) in service_env {
        task = task.env(name, value);
    }
    for name in &step.remove_env {
        task = task.env_remove(name);
    }
    task
}

fn execute(task: Task, parser: Option<&ParserConfig>, steps: &mut Vec<TaskResult>) -> Result<()> {
    print!("[RUN ] {} ... ", task.label);
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut result = task.run()?;
    if result.passed {
        if let Some(parser) = parser {
            let content = fs::read_to_string(&result.log).unwrap_or_default();
            let (count, minimum) = parse_result_count(&content, parser)?;
            if count < minimum {
                result.passed = false;
                result.detail = Some(format!(
                    "parsed {count} result(s), expected at least {minimum}"
                ));
            } else {
                let serial_reason = content
                    .lines()
                    .find_map(|line| line.strip_prefix("DEVRAIL_TEST_SERIAL_DATABASE_FACTS="));
                result.detail = Some(match serial_reason {
                    Some(reason) => format!("{count} result(s); serial database facts: {reason}"),
                    None => format!("{count} result(s)"),
                });
            }
        }
    }
    print_result_inline(&result);
    if result.cancelled {
        bail!("verification cancelled");
    }
    steps.push(result);
    Ok(())
}

fn parse_result_count(content: &str, parser: &ParserConfig) -> Result<(usize, usize)> {
    let ansi = Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]")?;
    let normalized = ansi.replace_all(content, "");
    match parser {
        ParserConfig::Regex {
            patterns,
            capture,
            minimum,
        } => {
            let mut count = 0;
            for pattern in patterns {
                let regex = Regex::new(pattern)?;
                count += regex
                    .captures_iter(&normalized)
                    .filter_map(|captures| captures.get(*capture)?.as_str().parse::<usize>().ok())
                    .sum::<usize>();
            }
            Ok((count, *minimum))
        }
    }
}

fn print_result(result: &TaskResult) {
    let marker = if result.passed { "PASS" } else { "FAIL" };
    println!("[{marker}] {} ({} ms)", result.label, result.duration_ms);
    if !result.passed && !result.log.is_empty() {
        println!("       log: {}", result.log);
    }
}

fn print_result_inline(result: &TaskResult) {
    let marker = if result.passed { "PASS" } else { "FAIL" };
    println!("{marker} ({} ms)", result.duration_ms);
    if !result.passed {
        println!("       log: {}", result.log);
    }
}

fn log(project: &Project, name: &str) -> std::path::PathBuf {
    project.reports.join("logs").join(name)
}

pub fn explicit_scope(components: &[String]) -> ScopeResult {
    ScopeResult {
        mode: "components".to_string(),
        changed_files: Vec::new(),
        components: components.iter().cloned().collect::<BTreeSet<_>>(),
        unmatched_files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FlowConfig, ServiceConfig};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn configurable_regex_parser_counts_multiple_outputs() {
        let parser = ParserConfig::Regex {
            patterns: vec![r"(?m)^running ([0-9]+) tests?$".into()],
            capture: 1,
            minimum: 1,
        };
        let log = "running 3 tests\n...\nrunning 2 tests\n";
        assert_eq!(parse_result_count(log, &parser).expect("count"), (5, 1));
    }

    #[test]
    fn a_new_test_framework_can_supply_its_own_pattern() {
        let parser = ParserConfig::Regex {
            patterns: vec![r"passed: ([0-9]+)".into()],
            capture: 1,
            minimum: 2,
        };
        assert_eq!(
            parse_result_count("passed: 7", &parser).expect("count"),
            (7, 2)
        );
    }

    #[test]
    fn regex_parser_ignores_ansi_color_sequences() {
        let parser = ParserConfig::Regex {
            patterns: vec![r"Tests\s+([0-9]+) passed".into()],
            capture: 1,
            minimum: 1,
        };
        let log = "\u{1b}[1mTests\u{1b}[22m  \u{1b}[32m58 passed\u{1b}[39m";
        assert_eq!(parse_result_count(log, &parser).expect("count"), (58, 1));
    }

    #[test]
    fn service_failure_does_not_skip_unrelated_steps() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("arc-flow-verify-{unique}"));
        crate::preset::init(&root, "generic", false).expect("initialize fixture");
        let flow_path = root.join(".arc-flow/flow.toml");
        let source = fs::read_to_string(&flow_path).expect("read fixture config");
        let mut config: FlowConfig = toml::from_str(&source).expect("parse fixture config");
        let source_env = format!("ARC_FLOW_MISSING_{unique}");
        assert!(std::env::var_os(&source_env).is_none());
        config.services.insert(
            "missing-service".into(),
            ServiceConfig::Environment {
                source_env,
                inject_env: "TEST_SERVICE_URL".into(),
            },
        );
        config.steps[0].services = vec!["missing-service".into()];
        config.steps[1].profiles.insert("full".into());
        fs::write(
            &flow_path,
            toml::to_string_pretty(&config).expect("serialize fixture config"),
        )
        .expect("write fixture config");
        let git = crate::process::capture("git", &["init".into()], &root, Duration::from_secs(5))
            .expect("initialize Git fixture");
        assert!(git.status.success());
        let project = Project::discover(Some(root.clone()), None).expect("discover fixture");

        let report =
            run(&project, ScopeResult::all(&project), "full", false).expect("verify fixture");

        assert!(!report.passed);
        assert!(report
            .steps
            .iter()
            .any(|step| step.label == "staged Git whitespace check" && step.passed));
        fs::remove_dir_all(root).ok();
    }
}
