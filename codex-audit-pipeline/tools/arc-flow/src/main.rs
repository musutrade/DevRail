mod audit;
mod config;
mod doctor;
mod preset;
mod process;
mod project;
mod scope;
mod secrets;
mod service;
mod verify;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use project::Project;
use scope::ScopeMode;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "arc-flow",
    version,
    about = "Configurable development workflow and architecture guard",
    arg_required_else_help = true,
    after_help = "Examples:\n  arc-flow presets\n  arc-flow init --preset rust-api\n  arc-flow doctor\n  arc-flow verify --all\n  cargo flow verify --components backend,frontend"
)]
struct Cli {
    /// Override automatic project root discovery.
    #[arg(long, global = true, value_name = "PATH")]
    project_root: Option<PathBuf>,

    /// Override the repository workflow configuration file.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check local tools, configuration, Git, and test database access.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Treat warnings as failures.
        #[arg(long)]
        strict: bool,
    },
    /// Show changed files and the verification components they select.
    Scope {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Scan file names for high-confidence credential patterns.
    Secrets {
        /// Scan the staged snapshot instead of the working tree.
        #[arg(long)]
        staged: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run deterministic architecture rules and write review_context reports.
    Audit {
        /// Emit the complete audit report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run gates and configured steps for the selected profile and components.
    #[command(visible_alias = "check")]
    Verify {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Override scope detection with a comma-separated component list.
        #[arg(
            long,
            value_delimiter = ',',
            num_args = 1..,
            conflicts_with_all = ["staged", "all", "base"]
        )]
        components: Vec<String>,
        /// Select any profile declared by configured steps.
        #[arg(long, value_name = "PROFILE")]
        profile: Option<String>,
    },
    /// Run the staged, fast verification profile used by pre-commit.
    Hook,
    /// Extract one trace's error context from JSON Lines logs.
    ParseLogs {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Validate or inspect the repository workflow configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run one configured full-profile step after secrets and audit gates.
    Step {
        /// Step id from flow.toml, for example api.clippy.
        id: String,
    },
    /// Initialize .arc-flow configuration from an embedded preset.
    Init {
        #[arg(long, default_value = "generic")]
        preset: String,
        /// Replace existing .arc-flow configuration files.
        #[arg(long)]
        force: bool,
    },
    /// List embedded project presets.
    Presets,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Validate configuration, environment overrides, and protected steps.
    Check,
    /// Print the source or effective configuration.
    Print {
        /// Include environment overrides in the rendered TOML.
        #[arg(long)]
        resolved: bool,
    },
    /// Convert a schema v1 flow.toml to .arc-flow/flow.toml schema v2.
    Migrate {
        #[arg(long, value_name = "PATH")]
        input: Option<PathBuf>,
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Args)]
struct ScopeArgs {
    /// Inspect only staged files.
    #[arg(long, conflicts_with_all = ["all", "base"])]
    staged: bool,
    /// Select every verification component.
    #[arg(long, conflicts_with_all = ["staged", "base"])]
    all: bool,
    /// Inspect committed changes in REF...HEAD.
    #[arg(long, value_name = "REF", conflicts_with_all = ["staged", "all"])]
    base: Option<String>,
}

impl ScopeArgs {
    fn mode(&self) -> ScopeMode {
        if self.staged {
            ScopeMode::Staged
        } else if self.all {
            ScopeMode::All
        } else if let Some(reference) = &self.base {
            ScopeMode::Base(reference.clone())
        } else {
            ScopeMode::WorkingTree
        }
    }
}

fn main() -> ExitCode {
    process::install_signal_handlers();
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("ERROR: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let cli = Cli::parse();
    if let Commands::Init {
        preset: preset_name,
        force,
    } = &cli.command
    {
        let target = cli
            .project_root
            .clone()
            .unwrap_or(std::env::current_dir().context("read current directory")?);
        preset::init(&target, preset_name, *force)?;
        return Ok(true);
    }
    if matches!(cli.command, Commands::Presets) {
        preset::print_presets();
        return Ok(true);
    }
    if let Commands::Config {
        action:
            ConfigAction::Migrate {
                input,
                output,
                force,
            },
    } = &cli.command
    {
        let root = cli
            .project_root
            .clone()
            .unwrap_or(std::env::current_dir().context("read current directory")?);
        preset::migrate(
            &root,
            input.clone().or_else(|| cli.config.clone()),
            output.clone(),
            *force,
        )?;
        return Ok(true);
    }
    let project = Project::discover(cli.project_root, cli.config)?;
    project.prepare()?;

    match cli.command {
        Commands::Doctor { json, strict } => {
            let report = doctor::run(&project)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                report.print();
            }
            Ok(report.failures == 0 && (!strict || report.warnings == 0))
        }
        Commands::Scope { scope: args, json } => {
            let result = scope::detect(&project, &args.mode())?;
            result.write_reports(&project)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_scope(&result);
            }
            Ok(true)
        }
        Commands::Secrets { staged, json } => {
            let mode = if staged {
                secrets::SecretMode::Staged
            } else {
                secrets::SecretMode::WorkingTree
            };
            let findings = secrets::scan(&project, mode)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "passed": findings.is_empty(),
                        "findings": findings,
                    }))?
                );
            } else if findings.is_empty() {
                println!("Secret scan passed");
            } else {
                eprintln!("Secret scan failed in {} file(s):", findings.len());
                for file in &findings {
                    eprintln!("  {file}");
                }
                eprintln!("Remove and revoke each credential before continuing.");
            }
            Ok(findings.is_empty())
        }
        Commands::Audit { json } => {
            let outcome = audit::run(&project.root, &project.audit_config, &project.reports, json)?;
            if !json {
                println!(
                    "Audit: {} violation(s), {} blocker(s), {} error(s), {} warning(s)",
                    outcome.total_violations,
                    outcome.blocker_count,
                    outcome.error_count,
                    outcome.warning_count
                );
                println!("Report: {}", outcome.report_file.display());
            }
            Ok(outcome.total_violations == 0)
        }
        Commands::Verify {
            scope: args,
            components,
            profile,
        } => {
            let selected = if components.is_empty() {
                scope::detect(&project, &args.mode())?
            } else {
                let known = project.config.components();
                for component in &components {
                    if !known.contains(component) {
                        bail!("unknown component {component:?}");
                    }
                }
                verify::explicit_scope(&components)
            };
            let profile = profile.unwrap_or_else(|| project.config.project.default_profile.clone());
            Ok(verify::run(&project, selected, &profile, false)?.passed)
        }
        Commands::Hook => {
            let selected = scope::detect(&project, &ScopeMode::Staged)?;
            Ok(verify::run(
                &project,
                selected,
                &project.config.project.hook_profile,
                true,
            )?
            .passed)
        }
        Commands::ParseLogs { input, output } => {
            audit::parse_logs(&input, &output)
                .with_context(|| format!("parse log file {}", input.display()))?;
            println!("Error context: {}", output.display());
            Ok(true)
        }
        Commands::Config { action } => match action {
            ConfigAction::Check => {
                println!("Configuration valid: {}", project.config_path.display());
                println!("Schema version: {}", project.config.version);
                println!(
                    "Components: {}",
                    project
                        .config
                        .components()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                println!(
                    "Profiles: {}",
                    project
                        .config
                        .steps
                        .iter()
                        .flat_map(|step| step.profiles.iter().cloned())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                println!("Verification steps: {}", project.config.steps.len());
                Ok(true)
            }
            ConfigAction::Print { resolved } => {
                if resolved {
                    println!("{}", toml::to_string_pretty(&project.config)?);
                } else {
                    print!(
                        "{}",
                        std::fs::read_to_string(&project.config_path).with_context(|| format!(
                            "read workflow config {}",
                            project.config_path.display()
                        ))?
                    );
                }
                Ok(true)
            }
            ConfigAction::Migrate { .. } => unreachable!("handled before project discovery"),
        },
        Commands::Step { id } => Ok(verify::run_step(&project, &id)?.passed),
        Commands::Init { .. } | Commands::Presets => {
            unreachable!("handled before project discovery")
        }
    }
}

fn print_scope(scope: &scope::ScopeResult) {
    println!("Scope: {}", scope.mode);
    println!("Changed files: {}", scope.changed_files.len());
    for file in &scope.changed_files {
        println!("  {file}");
    }
    if !scope.unmatched_files.is_empty() {
        println!("Unmatched files: {}", scope.unmatched_files.len());
        for file in &scope.unmatched_files {
            println!("  {file}");
        }
    }
    let components = scope
        .components
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "Components: {}",
        if components.is_empty() {
            "none"
        } else {
            &components
        }
    );
}
