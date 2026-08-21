use crate::project::resolve_repo_path;
use anyhow::{bail, Context, Result};
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

const AUDIT_CONFIG_VERSION: u32 = 2;
const AUDIT_MIGRATION_GUIDE: &str = "codex-audit-pipeline/docs/configuration.md#audit-v2-migration";

// ============================================================
// 配置结构体（与 .codex/audit.toml 对应）
// ============================================================
#[derive(Debug, Default, Deserialize, Clone)]
struct PathsConfig {
    #[serde(default)]
    exclude: Vec<String>,
    /// 路径别名表，例如 backend = "backend"；规则里写别名即可
    #[serde(flatten)]
    aliases: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    engine: EngineConfig,
    #[serde(default)]
    paths: PathsConfig,
    #[serde(default)]
    hard_rules: Vec<HardRule>,
    #[serde(default)]
    arch_rules: Vec<ArchRule>,
}

fn contains_legacy_string_allowlist(config: &toml::Value) -> bool {
    ["hard_rules", "arch_rules"].iter().any(|rules_key| {
        config
            .get(rules_key)
            .and_then(toml::Value::as_array)
            .is_some_and(|rules| {
                rules.iter().any(|rule| {
                    rule.get("allowlist")
                        .and_then(toml::Value::as_array)
                        .is_some_and(|entries| entries.iter().any(toml::Value::is_str))
                })
            })
    })
}

fn parse_audit_config(source: &str) -> Result<Config> {
    let raw: toml::Value = toml::from_str(source).context("parse audit config TOML")?;
    let table = raw
        .as_table()
        .context("audit config must be a top-level TOML table")?;
    let version = match table.get("version") {
        Some(value) => value.as_integer().context(
            "audit config schema version must be an integer; see the audit v2 migration guide",
        )?,
        None => bail!(
            "audit config schema version is missing; migrate to schema v{AUDIT_CONFIG_VERSION}: \
             add `version = {AUDIT_CONFIG_VERSION}`, add `[engine]`, and convert string allowlist \
             entries to explicit `path-prefix` or `regex` entries; see {AUDIT_MIGRATION_GUIDE}"
        ),
    };
    if version != i64::from(AUDIT_CONFIG_VERSION) {
        bail!(
            "unsupported audit config schema version {version}; expected \
             {AUDIT_CONFIG_VERSION}; see {AUDIT_MIGRATION_GUIDE}"
        );
    }
    if !table.contains_key("engine") {
        bail!(
            "audit config schema v{AUDIT_CONFIG_VERSION} requires `[engine]`; copy the engine \
             defaults and comment syntax from the current preset; see {AUDIT_MIGRATION_GUIDE}"
        );
    }
    if contains_legacy_string_allowlist(&raw) {
        bail!(
            "audit config schema v{AUDIT_CONFIG_VERSION} no longer accepts string allowlist \
             entries; replace each string with `{{ kind = \"path-prefix\", path = \"...\" }}` or \
             `{{ kind = \"regex\", pattern = \"...\" }}`; see {AUDIT_MIGRATION_GUIDE}"
        );
    }

    toml::from_str(source).with_context(|| {
        format!("parse audit config schema v{AUDIT_CONFIG_VERSION}; see {AUDIT_MIGRATION_GUIDE}")
    })
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct EngineConfig {
    ignore_filename: String,
    json_report_filename: String,
    markdown_report_filename: String,
    markdown_max_bytes: usize,
    markdown_occurrences_per_rule: usize,
    #[serde(default)]
    comment_syntax: HashMap<String, CommentSyntax>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct CommentSyntax {
    #[serde(default)]
    line: Vec<String>,
    #[serde(default)]
    block: Vec<BlockCommentSyntax>,
    #[serde(default)]
    strings: Vec<StringSyntax>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct BlockCommentSyntax {
    start: String,
    end: String,
    #[serde(default)]
    nested: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct StringSyntax {
    start: String,
    end: String,
    #[serde(default)]
    escape: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum AllowlistEntry {
    PathPrefix { path: String },
    Regex { pattern: String },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct HardRule {
    name: String,
    severity: String,
    paths: Vec<String>,
    extensions: Vec<String>,
    patterns: Vec<String>,
    #[serde(default)]
    exclude_patterns: Vec<String>,
    #[serde(default)]
    allowlist: Vec<AllowlistEntry>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ArchRule {
    name: String,
    layer: String,
    paths: Vec<String>,
    extensions: Vec<String>,
    forbidden_patterns: Vec<String>,
    #[serde(default)]
    allowed_patterns: Vec<String>,
    suggestion: String,
    #[serde(default)]
    exclude_patterns: Vec<String>,
    #[serde(default)]
    allowlist: Vec<AllowlistEntry>,
}

// ============================================================
// 违规结构体
// ============================================================
#[derive(Debug, Clone)]
struct Violation {
    file: PathBuf,
    line: usize,
    content: String,
    rule_name: String,
}

#[derive(Debug, Clone)]
struct ArchViolation {
    file: PathBuf,
    line: usize,
    content: String,
    rule_name: String,
}

// ============================================================
// 日志解析模块
// ============================================================
mod log_parser {
    use super::*;
    use serde_json::Value;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    fn extract_trace_id(json: &Value) -> Option<String> {
        trace_id_field(json)
            .or_else(|| json.get("fields").and_then(trace_id_field))
            .or_else(|| json.get("data").and_then(trace_id_field))
            .or_else(|| json.get("span").and_then(trace_id_field))
            .or_else(|| {
                json.get("spans")
                    .and_then(Value::as_array)
                    .and_then(|spans| spans.iter().rev().find_map(trace_id_field))
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn trace_id_field(json: &Value) -> Option<&Value> {
        json.get("trace_id").or_else(|| json.get("request_id"))
    }

    fn level_of(json: &Value) -> String {
        json.get("level")
            .or_else(|| json.get("severity"))
            .and_then(|v| v.as_str())
            .unwrap_or("INFO")
            .to_uppercase()
    }

    pub fn extract_error_context(input_path: &str, output_path: &str) -> Result<()> {
        let file = File::open(input_path)?;
        let reader = BufReader::new(file);

        let mut error_trace_id = String::new();
        let mut last_trace_id = String::new();
        let mut structured_logs: Vec<Value> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(json) = serde_json::from_str::<Value>(&line) {
                if let Some(tid) = extract_trace_id(&json) {
                    last_trace_id = tid.clone();
                    // 优先取第一条 ERROR 日志所在的 trace_id（比"最后一条"可靠）
                    if error_trace_id.is_empty() && level_of(&json) == "ERROR" {
                        error_trace_id = tid;
                    }
                }
                structured_logs.push(json);
            }
        }

        let target_trace_id = if error_trace_id.is_empty() {
            last_trace_id
        } else {
            error_trace_id
        };

        if target_trace_id.is_empty() {
            eprintln!("⚠️ 未找到 trace_id，降级输出原始日志尾部 30 行");
            let last_lines = get_last_n_lines(input_path, 30)?;
            fs::write(output_path, last_lines)?;
            return Ok(());
        }

        let mut output = Vec::new();
        for log in &structured_logs {
            if extract_trace_id(log).as_deref() != Some(target_trace_id.as_str()) {
                continue;
            }
            let timestamp = log
                .get("timestamp")
                .or_else(|| log.get("time"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let level = level_of(log);
            let target = log
                .get("target")
                .or_else(|| log.get("module"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fields = log.get("fields").or_else(|| log.get("data"));
            let msg = fields
                .and_then(|f| f.get("message").or_else(|| f.get("msg")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let error = fields
                .and_then(|f| f.get("error"))
                .or_else(|| log.get("error"))
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_default();

            let compact = serde_json::json!({
                "timestamp": timestamp,
                "level": level,
                "target": target,
                "msg": msg,
                "error": error,
                "trace_id": target_trace_id,
            });
            output.push(compact);
        }

        // 以第一条 ERROR 为中心保留上下文，避免长请求把根因截掉。
        if output.len() > 30 {
            let error_index = output
                .iter()
                .position(|entry| entry["level"] == "ERROR")
                .unwrap_or(output.len() - 1);
            let mut start = error_index.saturating_sub(20);
            let end = (start + 30).min(output.len());
            start = end.saturating_sub(30);
            output = output[start..end].to_vec();
        }

        let json_output = serde_json::to_string_pretty(&output)?;
        fs::write(output_path, json_output)?;
        eprintln!(
            "✅ 结构化日志已提取: {} ({} 条, trace_id={})",
            output_path,
            output.len(),
            target_trace_id
        );
        Ok(())
    }

    fn get_last_n_lines(path: &str, n: usize) -> Result<String> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().collect::<std::result::Result<Vec<_>, _>>()?;
        let start = lines.len().saturating_sub(n);
        Ok(lines[start..].join("\n"))
    }
}

// ============================================================
// 核心扫描函数
// ============================================================
fn compile_regexes(patterns: &[String]) -> Result<Vec<Regex>> {
    patterns
        .iter()
        .map(|pattern| {
            RegexBuilder::new(pattern)
                .multi_line(true)
                .build()
                .with_context(|| format!("invalid audit regex {pattern:?}"))
        })
        .collect()
}

fn resolve_rule_roots(
    project_root: &Path,
    entries: &[String],
    aliases: &HashMap<String, String>,
    rule_name: &str,
) -> Result<Vec<PathBuf>> {
    if entries.is_empty() {
        bail!("audit rule {rule_name:?} requires at least one path");
    }
    entries
        .iter()
        .map(|entry| aliases.get(entry).unwrap_or(entry))
        .map(|entry| {
            resolve_repo_path(
                project_root,
                Path::new(entry),
                &format!("audit rule {rule_name:?} path"),
                true,
            )
        })
        .collect()
}

fn resolve_excludes(project_root: &Path, entries: &[String]) -> Result<Vec<PathBuf>> {
    entries
        .iter()
        .map(|entry| {
            resolve_repo_path(project_root, Path::new(entry), "audit excluded path", false)
        })
        .collect()
}

fn source_line_starts(content: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            content
                .match_indices('\n')
                .map(|(newline_offset, _)| newline_offset + 1),
        )
        .collect()
}

fn source_line_at<'a>(
    content: &'a str,
    line_starts: &[usize],
    match_start: usize,
) -> (usize, &'a str, usize) {
    let line_index = line_starts
        .partition_point(|line_start| *line_start <= match_start)
        .saturating_sub(1);
    let line_start = line_starts[line_index];
    let line_end = content[line_start..]
        .find('\n')
        .map(|offset| line_start + offset)
        .unwrap_or(content.len());
    (
        line_index + 1,
        &content[line_start..line_end],
        match_start.saturating_sub(line_start),
    )
}

enum LexicalState<'a> {
    Code,
    String(&'a StringSyntax),
    LineComment {
        start: usize,
    },
    BlockComment {
        syntax: &'a BlockCommentSyntax,
        start: usize,
        depth: usize,
    },
}

fn token_at(bytes: &[u8], offset: usize, token: &str) -> bool {
    bytes[offset..].starts_with(token.as_bytes())
}

fn comment_ranges(content: &str, syntax: &CommentSyntax) -> Vec<Range<usize>> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut state = LexicalState::Code;
    let mut offset = 0;

    while offset < bytes.len() {
        match state {
            LexicalState::Code => {
                if let Some(string) = syntax
                    .strings
                    .iter()
                    .filter(|string| token_at(bytes, offset, &string.start))
                    .max_by_key(|string| string.start.len())
                {
                    offset += string.start.len();
                    state = LexicalState::String(string);
                } else if let Some(block) = syntax
                    .block
                    .iter()
                    .filter(|block| token_at(bytes, offset, &block.start))
                    .max_by_key(|block| block.start.len())
                {
                    let start = offset;
                    offset += block.start.len();
                    state = LexicalState::BlockComment {
                        syntax: block,
                        start,
                        depth: 1,
                    };
                } else if let Some(line) = syntax
                    .line
                    .iter()
                    .filter(|line| token_at(bytes, offset, line))
                    .max_by_key(|line| line.len())
                {
                    let start = offset;
                    offset += line.len();
                    state = LexicalState::LineComment { start };
                } else {
                    offset += 1;
                }
            }
            LexicalState::String(string) => {
                if string
                    .escape
                    .as_deref()
                    .is_some_and(|escape| token_at(bytes, offset, escape))
                {
                    offset += string.escape.as_deref().map_or(0, str::len);
                    offset = (offset + 1).min(bytes.len());
                } else if token_at(bytes, offset, &string.end) {
                    offset += string.end.len();
                    state = LexicalState::Code;
                } else {
                    offset += 1;
                }
            }
            LexicalState::LineComment { start } => {
                if bytes[offset] == b'\n' {
                    ranges.push(start..offset);
                    state = LexicalState::Code;
                }
                offset += 1;
            }
            LexicalState::BlockComment {
                syntax: block,
                start,
                mut depth,
            } => {
                if block.nested && token_at(bytes, offset, &block.start) {
                    depth += 1;
                    offset += block.start.len();
                    state = LexicalState::BlockComment {
                        syntax: block,
                        start,
                        depth,
                    };
                } else if token_at(bytes, offset, &block.end) {
                    depth -= 1;
                    offset += block.end.len();
                    if depth == 0 {
                        ranges.push(start..offset);
                        state = LexicalState::Code;
                    } else {
                        state = LexicalState::BlockComment {
                            syntax: block,
                            start,
                            depth,
                        };
                    }
                } else {
                    offset += 1;
                    state = LexicalState::BlockComment {
                        syntax: block,
                        start,
                        depth,
                    };
                }
            }
        }
    }

    match state {
        LexicalState::LineComment { start } | LexicalState::BlockComment { start, .. } => {
            ranges.push(start..bytes.len())
        }
        LexicalState::Code | LexicalState::String(_) => {}
    }
    ranges
}

fn is_comment_offset(ranges: &[Range<usize>], offset: usize) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[index - 1].contains(&offset)
}

fn is_allowlisted(path: &Path, project_root: &Path, allowlist: &[AllowlistEntry]) -> bool {
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let path_str = relative.to_str().unwrap_or("");
    allowlist.iter().any(|entry| match entry {
        AllowlistEntry::PathPrefix { path } => relative.starts_with(Path::new(path)),
        AllowlistEntry::Regex { pattern } => Regex::new(pattern)
            .map(|re| re.is_match(path_str))
            .unwrap_or(false),
    })
}

fn scan_files(
    project_root: &Path,
    root_paths: &[PathBuf],
    exclude_dirs: &[PathBuf],
    rule: &HardRule,
    engine: &EngineConfig,
) -> Result<Vec<Violation>> {
    if root_paths.is_empty() || rule.patterns.is_empty() {
        return Ok(Vec::new());
    }

    let regexes = compile_regexes(&rule.patterns)?;
    let exclude_regexes = compile_regexes(&rule.exclude_patterns)?;

    let rule_name = rule.name.clone();
    let mut walk_builder = WalkBuilder::new(root_paths[0].clone());
    for root_path in root_paths.iter().skip(1) {
        walk_builder.add(root_path);
    }
    let entries = walk_builder
        .add_custom_ignore_filename(&engine.ignore_filename)
        .follow_links(false)
        .build()
        .collect::<std::result::Result<Vec<_>, ignore::Error>>()?;
    let violations = entries
        .into_par_iter()
        .filter(|entry| {
            let path = entry.path();
            if path.is_dir() {
                return false;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !rule.extensions.contains(&ext.to_string()) {
                    return false;
                }
            } else {
                return false;
            }
            for excl in exclude_dirs {
                if path.starts_with(excl) {
                    return false;
                }
            }
            let relative = path.strip_prefix(project_root).unwrap_or(path);
            let path_str = relative.to_str().unwrap_or("");
            if exclude_regexes.iter().any(|re| re.is_match(path_str)) {
                return false;
            }
            !is_allowlisted(path, project_root, &rule.allowlist)
        })
        .map(|entry| -> Result<Vec<Violation>> {
            let path = entry.path();
            let content = fs::read_to_string(path)
                .with_context(|| format!("read audit source {}", path.display()))?;
            let mut violations = Vec::new();
            let mut reported_lines = HashSet::new();
            let line_starts = source_line_starts(&content);
            let extension = path.extension().and_then(|value| value.to_str());
            let comments = extension
                .and_then(|extension| engine.comment_syntax.get(extension))
                .map(|syntax| comment_ranges(&content, syntax))
                .unwrap_or_default();

            for (idx, re) in regexes.iter().enumerate() {
                for matched in re.find_iter(&content) {
                    let (line_number, line, _) =
                        source_line_at(&content, &line_starts, matched.start());
                    if is_comment_offset(&comments, matched.start())
                        || !reported_lines.insert(line_number)
                    {
                        continue;
                    }
                    violations.push(Violation {
                        file: path
                            .strip_prefix(project_root)
                            .unwrap_or(path)
                            .to_path_buf(),
                        line: line_number,
                        content: line.trim().to_string(),
                        rule_name: format!("{}:{}", rule_name, rule.patterns[idx]),
                    });
                }
            }
            violations.sort_by_key(|violation| violation.line);
            Ok(violations)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(violations.into_iter().flatten().collect())
}

fn scan_arch_rules(
    project_root: &Path,
    config: &Config,
    exclude_dirs: &[PathBuf],
) -> Result<Vec<ArchViolation>> {
    let mut all_violations = Vec::new();

    for rule in &config.arch_rules {
        let root_paths =
            resolve_rule_roots(project_root, &rule.paths, &config.paths.aliases, &rule.name)?;
        let extensions = rule.extensions.clone();
        let patterns = rule.forbidden_patterns.clone();
        let allowed_patterns = rule.allowed_patterns.clone();
        let exclude_patterns = rule.exclude_patterns.clone();
        let allowlist = rule.allowlist.clone();

        if patterns.is_empty() {
            continue;
        }

        let regexes = compile_regexes(&patterns)?;
        let allowed_regexes = compile_regexes(&allowed_patterns)?;
        let exclude_regexes = compile_regexes(&exclude_patterns)?;

        let rule_name = rule.name.clone();
        let mut walk_builder = WalkBuilder::new(root_paths[0].clone());
        for root_path in root_paths.iter().skip(1) {
            walk_builder.add(root_path);
        }
        let entries = walk_builder
            .add_custom_ignore_filename(&config.engine.ignore_filename)
            .follow_links(false)
            .build()
            .collect::<std::result::Result<Vec<_>, ignore::Error>>()?;
        let rule_violations = entries
            .into_par_iter()
            .filter(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    return false;
                }
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if !extensions.contains(&ext.to_string()) {
                        return false;
                    }
                } else {
                    return false;
                }
                for excl in exclude_dirs {
                    if path.starts_with(excl) {
                        return false;
                    }
                }
                let relative = path.strip_prefix(project_root).unwrap_or(path);
                let path_str = relative.to_str().unwrap_or("");
                if exclude_regexes.iter().any(|re| re.is_match(path_str)) {
                    return false;
                }
                !is_allowlisted(path, project_root, &allowlist)
            })
            .map(|entry| -> Result<Vec<ArchViolation>> {
                let path = entry.path();
                let content = fs::read_to_string(path)
                    .with_context(|| format!("read audit source {}", path.display()))?;
                let mut violations = Vec::new();
                let mut reported_lines = HashSet::new();
                let line_starts = source_line_starts(&content);
                let extension = path.extension().and_then(|value| value.to_str());
                let comments = extension
                    .and_then(|extension| config.engine.comment_syntax.get(extension))
                    .map(|syntax| comment_ranges(&content, syntax))
                    .unwrap_or_default();

                for re in &regexes {
                    for matched in re.find_iter(&content) {
                        let (line_number, line, _) =
                            source_line_at(&content, &line_starts, matched.start());
                        if is_comment_offset(&comments, matched.start())
                            || allowed_regexes.iter().any(|allowed| allowed.is_match(line))
                            || !reported_lines.insert(line_number)
                        {
                            continue;
                        }
                        violations.push(ArchViolation {
                            file: path
                                .strip_prefix(project_root)
                                .unwrap_or(path)
                                .to_path_buf(),
                            line: line_number,
                            content: line.trim().to_string(),
                            rule_name: rule_name.clone(),
                        });
                    }
                }
                violations.sort_by_key(|violation| violation.line);
                Ok(violations)
            })
            .collect::<Result<Vec<_>>>()?;

        all_violations.extend(rule_violations.into_iter().flatten());
    }

    Ok(all_violations)
}

// ============================================================
// 报告生成
// ============================================================
fn generate_markdown(
    config: &Config,
    hard_violations: &[Violation],
    arch_violations: &[ArchViolation],
) -> String {
    let mut output = String::new();
    let occurrence_limit = config.engine.markdown_occurrences_per_rule;

    output.push_str("=== 【自动化硬性约束扫描结果】 ===\n\n");

    for rule in &config.hard_rules {
        let rule_violations: Vec<&Violation> = hard_violations
            .iter()
            .filter(|v| v.rule_name.starts_with(&rule.name))
            .collect();

        let count = rule_violations.len();
        output.push_str(&format!(">> {}: 违规数量 {}\n", rule.name, count));

        if count > 0 {
            for v in rule_violations.iter().take(occurrence_limit) {
                output.push_str(&format!(
                    "    {}:{}: {}\n",
                    v.file.display(),
                    v.line,
                    v.content
                ));
            }
            if count > occurrence_limit {
                output.push_str(&format!("    ... 剩余 {} 处\n", count - occurrence_limit));
            }
        } else {
            output.push_str("  ✅ 未发现\n");
        }
        output.push('\n');
    }

    output.push_str("=== 【架构分层违规预扫描】 ===\n\n");

    for rule in &config.arch_rules {
        let violations: Vec<&ArchViolation> = arch_violations
            .iter()
            .filter(|v| v.rule_name == rule.name)
            .collect();
        let count = violations.len();

        output.push_str(&format!(">> {}: 违规数量 {}\n", rule.name, count));

        if count > 0 {
            for v in violations.iter().take(occurrence_limit) {
                output.push_str(&format!(
                    "    {}:{}: {}\n",
                    v.file.display(),
                    v.line,
                    v.content
                ));
            }
            if count > occurrence_limit {
                output.push_str(&format!("    ... 剩余 {} 处\n", count - occurrence_limit));
            }
            output.push_str(&format!("  💡 建议: {}\n", rule.suggestion));
        } else {
            output.push_str("  ✅ 未发现违规\n");
        }
        output.push('\n');
    }

    output
}

#[derive(Debug, Serialize)]
struct JsonOccurrence {
    file: String,
    line: usize,
    content: String,
}

#[derive(Debug, Serialize)]
struct JsonViolation {
    rule: String,
    severity: String,
    count: usize,
    occurrences: Vec<JsonOccurrence>,
}

#[derive(Debug, Serialize)]
struct JsonArchViolation {
    rule: String,
    layer: String,
    count: usize,
    suggestion: String,
    occurrences: Vec<JsonOccurrence>,
}

#[derive(Debug, Clone, Serialize)]
struct JsonSummary {
    total_violations: usize,
    blocker_count: usize,
    error_count: usize,
    warning_count: usize,
}

#[derive(Debug, Serialize)]
struct JsonReport {
    timestamp: String,
    hard_violations: Vec<JsonViolation>,
    arch_violations: Vec<JsonArchViolation>,
    summary: JsonSummary,
}

fn generate_report(
    config: &Config,
    hard_violations: &[Violation],
    arch_violations: &[ArchViolation],
) -> JsonReport {
    let mut hard_json = Vec::new();
    for rule in &config.hard_rules {
        let rule_violations: Vec<&Violation> = hard_violations
            .iter()
            .filter(|v| v.rule_name.starts_with(&rule.name))
            .collect();

        let occurrences: Vec<JsonOccurrence> = rule_violations
            .iter()
            .map(|v| JsonOccurrence {
                file: v.file.to_string_lossy().to_string(),
                line: v.line,
                content: v.content.clone(),
            })
            .collect();

        hard_json.push(JsonViolation {
            rule: rule.name.clone(),
            severity: rule.severity.clone(),
            count: occurrences.len(),
            occurrences,
        });
    }

    let mut arch_json = Vec::new();
    for rule in &config.arch_rules {
        let rule_violations: Vec<&ArchViolation> = arch_violations
            .iter()
            .filter(|v| v.rule_name == rule.name)
            .collect();

        let occurrences: Vec<JsonOccurrence> = rule_violations
            .iter()
            .map(|v| JsonOccurrence {
                file: v.file.to_string_lossy().to_string(),
                line: v.line,
                content: v.content.clone(),
            })
            .collect();

        arch_json.push(JsonArchViolation {
            rule: rule.name.clone(),
            layer: rule.layer.clone(),
            count: occurrences.len(),
            suggestion: rule.suggestion.clone(),
            occurrences,
        });
    }

    let total: usize = hard_json.iter().map(|v| v.count).sum::<usize>()
        + arch_json.iter().map(|v| v.count).sum::<usize>();
    let blocker_count: usize = hard_json
        .iter()
        .filter(|v| v.severity == "blocker")
        .map(|v| v.count)
        .sum();
    let error_count: usize = hard_json
        .iter()
        .filter(|v| v.severity == "error")
        .map(|v| v.count)
        .sum();
    let warning_count: usize = hard_json
        .iter()
        .filter(|v| v.severity == "warning")
        .map(|v| v.count)
        .sum();

    JsonReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        hard_violations: hard_json,
        arch_violations: arch_json,
        summary: JsonSummary {
            total_violations: total,
            blocker_count,
            error_count,
            warning_count,
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditOutcome {
    pub total_violations: usize,
    pub blocker_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub report_file: PathBuf,
}

fn validate_filename(value: &str, field: &str) -> Result<()> {
    let path = Path::new(value);
    if value.trim().is_empty() || path.components().count() != 1 || path.file_name().is_none() {
        bail!("audit engine {field} must be a filename without directory components");
    }
    Ok(())
}

fn validate_engine_config(engine: &EngineConfig) -> Result<()> {
    validate_filename(&engine.ignore_filename, "ignore_filename")?;
    validate_filename(&engine.json_report_filename, "json_report_filename")?;
    validate_filename(&engine.markdown_report_filename, "markdown_report_filename")?;
    if engine.json_report_filename == engine.markdown_report_filename {
        bail!("audit engine report filenames must be distinct");
    }
    if engine.markdown_max_bytes == 0 || engine.markdown_occurrences_per_rule == 0 {
        bail!("audit engine markdown limits must be positive");
    }
    for (extension, syntax) in &engine.comment_syntax {
        if extension.trim().is_empty()
            || syntax.line.iter().any(|token| token.is_empty())
            || syntax
                .block
                .iter()
                .any(|block| block.start.is_empty() || block.end.is_empty())
            || syntax.strings.iter().any(|string| {
                string.start.is_empty()
                    || string.end.is_empty()
                    || string.escape.as_ref().is_some_and(String::is_empty)
            })
        {
            bail!("audit engine comment syntax for {extension:?} contains an empty token");
        }
        if syntax
            .block
            .iter()
            .any(|block| block.nested && block.start == block.end)
        {
            bail!(
                "audit engine nested comment syntax for {extension:?} requires distinct delimiters"
            );
        }
    }
    Ok(())
}

fn validate_rule_extensions(
    engine: &EngineConfig,
    extensions: &[String],
    rule_name: &str,
) -> Result<()> {
    for extension in extensions {
        if extension.trim().is_empty() {
            bail!("audit rule {rule_name:?} contains an empty extension");
        }
        if !engine.comment_syntax.contains_key(extension) {
            bail!(
                "audit rule {rule_name:?} uses extension {extension:?} without \
                 `[engine.comment_syntax.{extension}]`; define its comments and string delimiters \
                 before scanning"
            );
        }
    }
    Ok(())
}

fn validate_audit_config(project_root: &Path, config: &Config) -> Result<Vec<PathBuf>> {
    if config.version != AUDIT_CONFIG_VERSION {
        bail!(
            "unsupported audit config schema version {}; expected {}",
            config.version,
            AUDIT_CONFIG_VERSION
        );
    }
    validate_engine_config(&config.engine)?;
    for (alias, path) in &config.paths.aliases {
        resolve_repo_path(
            project_root,
            Path::new(path),
            &format!("audit path alias {alias:?}"),
            false,
        )?;
    }
    let exclude_dirs = resolve_excludes(project_root, &config.paths.exclude)?;
    let mut names = HashSet::new();

    for rule in &config.hard_rules {
        if rule.name.trim().is_empty() || !names.insert(rule.name.as_str()) {
            bail!(
                "audit rule names must be non-empty and unique: {:?}",
                rule.name
            );
        }
        if !matches!(rule.severity.as_str(), "blocker" | "error" | "warning") {
            bail!(
                "audit rule {:?} has unsupported severity {:?}",
                rule.name,
                rule.severity
            );
        }
        if rule.extensions.is_empty() || rule.patterns.is_empty() {
            bail!(
                "audit rule {:?} requires extensions and patterns",
                rule.name
            );
        }
        validate_rule_extensions(&config.engine, &rule.extensions, &rule.name)?;
        resolve_rule_roots(project_root, &rule.paths, &config.paths.aliases, &rule.name)?;
        compile_regexes(&rule.patterns)?;
        compile_regexes(&rule.exclude_patterns)?;
        validate_allowlist(project_root, &rule.allowlist, &rule.name)?;
    }
    for rule in &config.arch_rules {
        if rule.name.trim().is_empty() || !names.insert(rule.name.as_str()) {
            bail!(
                "audit rule names must be non-empty and unique: {:?}",
                rule.name
            );
        }
        if rule.layer.trim().is_empty()
            || rule.suggestion.trim().is_empty()
            || rule.extensions.is_empty()
            || rule.forbidden_patterns.is_empty()
        {
            bail!(
                "architecture rule {:?} requires layer, suggestion, extensions, and forbidden_patterns",
                rule.name
            );
        }
        validate_rule_extensions(&config.engine, &rule.extensions, &rule.name)?;
        resolve_rule_roots(project_root, &rule.paths, &config.paths.aliases, &rule.name)?;
        compile_regexes(&rule.forbidden_patterns)?;
        compile_regexes(&rule.allowed_patterns)?;
        compile_regexes(&rule.exclude_patterns)?;
        validate_allowlist(project_root, &rule.allowlist, &rule.name)?;
    }
    Ok(exclude_dirs)
}

fn validate_allowlist(
    project_root: &Path,
    allowlist: &[AllowlistEntry],
    rule_name: &str,
) -> Result<()> {
    for entry in allowlist {
        match entry {
            AllowlistEntry::PathPrefix { path } => {
                resolve_repo_path(
                    project_root,
                    Path::new(path),
                    &format!("audit rule {rule_name:?} allowlist path"),
                    false,
                )?;
            }
            AllowlistEntry::Regex { pattern } => {
                Regex::new(pattern).with_context(|| {
                    format!("audit rule {rule_name:?} has invalid allowlist regex {pattern:?}")
                })?;
            }
        }
    }
    Ok(())
}

pub fn run(
    project_root: &Path,
    config_path: &Path,
    report_dir: &Path,
    emit_json: bool,
) -> Result<AuditOutcome> {
    let config_str = fs::read_to_string(config_path)
        .with_context(|| format!("read audit config {}", config_path.display()))?;
    let config = parse_audit_config(&config_str)
        .with_context(|| format!("parse audit config {}", config_path.display()))?;
    let exclude_dirs = validate_audit_config(project_root, &config)?;

    let mut all_hard_violations = Vec::new();
    for rule in &config.hard_rules {
        let root_paths =
            resolve_rule_roots(project_root, &rule.paths, &config.paths.aliases, &rule.name)?;
        let violations = scan_files(
            project_root,
            &root_paths,
            &exclude_dirs,
            rule,
            &config.engine,
        )?;
        all_hard_violations.extend(violations);
    }

    let arch_violations = scan_arch_rules(project_root, &config, &exclude_dirs)?;
    let report = generate_report(&config, &all_hard_violations, &arch_violations);
    let full_json = serde_json::to_string_pretty(&report)?;
    let outcome = AuditOutcome {
        total_violations: report.summary.total_violations,
        blocker_count: report.summary.blocker_count,
        error_count: report.summary.error_count,
        warning_count: report.summary.warning_count,
        report_file: report_dir.join(&config.engine.json_report_filename),
    };

    fs::create_dir_all(report_dir)?;
    fs::write(&outcome.report_file, &full_json)?;

    let markdown = generate_markdown(&config, &all_hard_violations, &arch_violations);
    let truncated = if markdown.len() > config.engine.markdown_max_bytes {
        let mut value = markdown;
        let mut boundary = config.engine.markdown_max_bytes;
        while !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        value.truncate(boundary);
        value.push_str(&format!(
            "\n\n... (report truncated to {} bytes; see {})",
            config.engine.markdown_max_bytes, config.engine.json_report_filename
        ));
        value
    } else {
        markdown
    };
    fs::write(
        report_dir.join(&config.engine.markdown_report_filename),
        truncated,
    )?;

    if emit_json {
        println!("{full_json}");
    }

    Ok(outcome)
}

pub fn parse_logs(input: &Path, output: &Path) -> Result<()> {
    log_parser::extract_error_context(&input.to_string_lossy(), &output.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time must be after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "arc-flow-auditor-{name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn child(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).expect("create child directory");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn configured_audit() -> Config {
        parse_audit_config(include_str!("../../../.codex/audit.toml"))
            .expect("project audit config must parse")
    }

    fn configured_hard_rule(name: &str) -> HardRule {
        configured_audit()
            .hard_rules
            .into_iter()
            .find(|rule| rule.name == name)
            .expect("configured hard rule must exist")
    }

    #[test]
    fn audit_schema_accepts_current_config_and_rejects_incompatible_inputs() {
        let current = include_str!("../presets/empty.audit.toml");
        assert_eq!(
            parse_audit_config(current)
                .expect("current audit preset must parse")
                .version,
            AUDIT_CONFIG_VERSION
        );

        let missing_version = current.replacen("version = 2\n", "", 1);
        let error = parse_audit_config(&missing_version)
            .expect_err("an unversioned audit config must fail closed");
        assert!(error.to_string().contains("add `version = 2`"));

        let unknown_version = current.replacen("version = 2", "version = 99", 1);
        let error = parse_audit_config(&unknown_version)
            .expect_err("an unknown audit schema must fail closed");
        assert!(error
            .to_string()
            .contains("unsupported audit config schema version 99"));

        let unknown_field = format!("unknown = true\n{current}");
        assert!(parse_audit_config(&unknown_field).is_err());
    }

    #[test]
    fn legacy_audit_shapes_receive_actionable_migration_errors() {
        let missing_engine = "version = 2\nhard_rules = []\narch_rules = []\n";
        let error = parse_audit_config(missing_engine)
            .expect_err("schema v2 requires an explicit engine configuration");
        assert!(error.to_string().contains("requires `[engine]`"));

        let legacy_allowlist = r#"
version = 2

[engine]
ignore_filename = ".auditignore"
json_report_filename = "review_context.json"
markdown_report_filename = "review_context.md"
markdown_max_bytes = 4096
markdown_occurrences_per_rule = 3

[engine.comment_syntax.rs]
line = ["//"]

[[hard_rules]]
name = "legacy"
severity = "error"
paths = ["src"]
extensions = ["rs"]
patterns = ["forbidden"]
allowlist = ["src/generated"]
"#;
        let error = parse_audit_config(legacy_allowlist)
            .expect_err("string allowlists must require an explicit migration");
        assert!(error
            .to_string()
            .contains("no longer accepts string allowlist"));
        assert!(error.to_string().contains("kind = \"path-prefix\""));
    }

    #[test]
    fn rule_extension_requires_configured_comment_syntax() {
        let test_dir = TestDir::new("missing-comment-syntax");
        let source = test_dir.child("src");
        fs::write(source.join("sample.go"), "package sample\n").expect("write Go fixture");
        let mut config = configured_audit();
        config.hard_rules = vec![HardRule {
            name: "Go rule".into(),
            severity: "error".into(),
            paths: vec!["src".into()],
            extensions: vec!["go".into()],
            patterns: vec!["forbidden".into()],
            exclude_patterns: Vec::new(),
            allowlist: Vec::new(),
        }];
        config.arch_rules.clear();

        let error = validate_audit_config(&test_dir.0, &config)
            .expect_err("an extension without lexical syntax must fail closed");
        assert!(error.to_string().contains("[engine.comment_syntax.go]"));
    }

    #[test]
    fn initialized_project_can_add_and_run_its_first_audit_rule() {
        let test_dir = TestDir::new("initialized-first-rule");
        crate::preset::init(&test_dir.0, "generic", false).expect("initialize project");
        let source = test_dir.child("src");
        fs::write(
            source.join("sample.rs"),
            "// forbidden_call()\nfn sample() { forbidden_call(); }\n",
        )
        .expect("write Rust fixture");
        let config_path = test_dir.0.join(".arc-flow/audit.toml");
        let mut config = fs::read_to_string(&config_path).expect("read initialized audit config");
        config.push_str(
            r#"

[[hard_rules]]
name = "first rule"
severity = "error"
paths = ["src"]
extensions = ["rs"]
patterns = ["forbidden_call"]
allowlist = []
exclude_patterns = []
"#,
        );
        fs::write(&config_path, config).expect("append first audit rule");

        let outcome = run(
            &test_dir.0,
            &config_path,
            &test_dir.0.join(".arc-flow/reports"),
            false,
        )
        .expect("run first audit rule");

        assert_eq!(outcome.total_violations, 1);
        assert_eq!(outcome.error_count, 1);
    }

    #[test]
    fn hard_rule_scans_every_configured_root() {
        let test_dir = TestDir::new("hard-roots");
        let first = test_dir.child("first");
        let second = test_dir.child("second");
        fs::write(first.join("one.rs"), "forbidden_call();\n").expect("write first fixture");
        fs::write(second.join("two.rs"), "forbidden_call();\n").expect("write second fixture");

        let roots = vec![first, second];
        let rule = HardRule {
            name: "test rule".to_string(),
            severity: "error".to_string(),
            paths: Vec::new(),
            extensions: vec!["rs".to_string()],
            patterns: vec!["forbidden_call".to_string()],
            exclude_patterns: Vec::new(),
            allowlist: Vec::new(),
        };
        let violations = scan_files(&test_dir.0, &roots, &[], &rule, &configured_audit().engine)
            .expect("scan fixture");

        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn hard_rule_detects_multiline_sensitive_logging() {
        let test_dir = TestDir::new("multiline-sensitive-log");
        let source = test_dir.child("src");
        fs::write(
            source.join("leak.rs"),
            concat!(
                "fn leak(password: &str, access_token: &str) {\n",
                "    tracing::error!(\n",
                "        password = password,\n",
                "        \"login failed\"\n",
                "    );\n",
                "    warn!(\n",
                "        ?access_token,\n",
                "        \"request failed\"\n",
                "    );\n",
                "    // tracing::error!(password = password);\n",
                "}\n",
            ),
        )
        .expect("write sensitive log fixture");
        let rule = configured_hard_rule("日志不得记录完整请求头或显式敏感字段");

        let violations = scan_files(
            &test_dir.0,
            &[source],
            &[],
            &rule,
            &configured_audit().engine,
        )
        .expect("scan sensitive log fixture");

        assert_eq!(violations.len(), 2);
        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            vec![2, 6]
        );
    }

    #[test]
    fn hard_rule_detects_multiline_and_dynamic_sql_surfaces() {
        let test_dir = TestDir::new("multiline-sql");
        let source = test_dir.child("src");
        fs::write(
            source.join("write.rs"),
            concat!(
                "const SQL: &str = r#\"\n",
                "UPDATE\n",
                "    users\n",
                "SET disabled = true\n",
                "\"#;\n",
            ),
        )
        .expect("write multiline SQL fixture");
        fs::write(
            source.join("raw.rs"),
            "sqlx::\n    raw_sql(\"SELECT 1\");\n// raw_sql(\"SELECT 2\");\n",
        )
        .expect("write raw SQL fixture");
        fs::write(
            source.join("builder.rs"),
            "QueryBuilder::<Postgres>\n    ::new(\"SELECT 1\");\n",
        )
        .expect("write query builder fixture");
        fs::write(
            source.join("write.sql"),
            "-- DELETE FROM ignored\nDELETE\nFROM sessions;\n",
        )
        .expect("write external SQL fixture");
        let rule = configured_hard_rule("SQL 写操作仅允许出现在 Repository/迁移/测试层");

        let violations = scan_files(
            &test_dir.0,
            &[source],
            &[],
            &rule,
            &configured_audit().engine,
        )
        .expect("scan SQL fixtures");

        assert_eq!(violations.len(), 4);
        assert!(violations
            .iter()
            .any(|violation| violation.file.ends_with("write.sql") && violation.line == 2));
    }

    #[test]
    fn architecture_rule_detects_multiline_raw_sql_and_query_builder() {
        let test_dir = TestDir::new("multiline-service-sql");
        let services = test_dir.child("services");
        fs::write(
            services.join("raw.rs"),
            "sqlx\n    ::\n    raw_sql(\"SELECT 1\");\n",
        )
        .expect("write service raw SQL fixture");
        fs::write(
            services.join("builder.rs"),
            "QueryBuilder::<Postgres>\n    ::new(\"SELECT 1\");\n",
        )
        .expect("write service query builder fixture");
        let mut service_rule = configured_audit()
            .arch_rules
            .into_iter()
            .find(|rule| rule.name == "Service 层不应包含 SQL 查询")
            .expect("service SQL rule must exist");
        service_rule.paths = vec!["services".to_string()];
        let config = Config {
            version: AUDIT_CONFIG_VERSION,
            engine: configured_audit().engine,
            paths: PathsConfig::default(),
            hard_rules: Vec::new(),
            arch_rules: vec![service_rule],
        };

        let violations =
            scan_arch_rules(&test_dir.0, &config, &[]).expect("scan service SQL fixtures");

        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn architecture_rule_scans_every_configured_root() {
        let test_dir = TestDir::new("arch-roots");
        let pages = test_dir.child("pages");
        let layout = test_dir.child("layout");
        fs::write(pages.join("page.ts"), "HttpClient\n").expect("write page fixture");
        fs::write(layout.join("layout.ts"), "HttpClient\n").expect("write layout fixture");

        let config = Config {
            version: AUDIT_CONFIG_VERSION,
            engine: configured_audit().engine,
            paths: PathsConfig {
                exclude: Vec::new(),
                aliases: HashMap::new(),
            },
            hard_rules: Vec::new(),
            arch_rules: vec![ArchRule {
                name: "component rule".to_string(),
                layer: "component".to_string(),
                paths: vec!["pages".into(), "layout".into()],
                extensions: vec!["ts".to_string()],
                forbidden_patterns: vec!["HttpClient".to_string()],
                allowed_patterns: Vec::new(),
                suggestion: "use a service".to_string(),
                exclude_patterns: Vec::new(),
                allowlist: Vec::new(),
            }],
        };

        assert_eq!(
            scan_arch_rules(&test_dir.0, &config, &[])
                .expect("scan config")
                .len(),
            2
        );
    }

    #[test]
    fn literal_allowlist_is_a_path_prefix_not_a_substring() {
        let allowlist = vec![AllowlistEntry::PathPrefix {
            path: "backend/src/repositories".to_string(),
        }];
        let root = Path::new("/repo");

        assert!(is_allowlisted(
            Path::new("/repo/backend/src/repositories/users.rs"),
            root,
            &allowlist
        ));
        assert!(!is_allowlisted(
            Path::new("/repo/backend/src/repositories_backup/users.rs"),
            root,
            &allowlist
        ));
    }

    #[test]
    fn regex_allowlist_is_explicit() {
        let allowlist = vec![AllowlistEntry::Regex {
            pattern: r"^backend/src/generated/.*\.rs$".to_string(),
        }];
        let root = Path::new("/repo");

        assert!(is_allowlisted(
            Path::new("/repo/backend/src/generated/users.rs"),
            root,
            &allowlist
        ));
        assert!(!is_allowlisted(
            Path::new("/repo/backend/src/services/users.rs"),
            root,
            &allowlist
        ));
    }

    #[test]
    fn configured_comment_syntax_handles_strings_and_block_comments() {
        let test_dir = TestDir::new("comment-syntax");
        let source = test_dir.child("src");
        fs::write(
            source.join("sample.rs"),
            concat!(
                "fn sample() {\n",
                "    let url = \"https://example.invalid\"; forbidden_call();\n",
                "    /* forbidden_call();\n",
                "       /* forbidden_call(); */\n",
                "    */\n",
                "    let marker = \"/*\"; forbidden_call();\n",
                "    // forbidden_call();\n",
                "}\n",
            ),
        )
        .expect("write comment fixture");
        let rule = HardRule {
            name: "comment rule".into(),
            severity: "error".into(),
            paths: Vec::new(),
            extensions: vec!["rs".into()],
            patterns: vec!["forbidden_call".into()],
            exclude_patterns: Vec::new(),
            allowlist: Vec::new(),
        };

        let violations = scan_files(
            &test_dir.0,
            &[source],
            &[],
            &rule,
            &configured_audit().engine,
        )
        .expect("scan comment fixture");

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            vec![2, 6]
        );
    }

    #[test]
    fn missing_rule_root_fails_closed() {
        let test_dir = TestDir::new("missing-root");
        let config = Config {
            version: AUDIT_CONFIG_VERSION,
            engine: configured_audit().engine,
            paths: PathsConfig::default(),
            hard_rules: vec![HardRule {
                name: "missing".into(),
                severity: "blocker".into(),
                paths: vec!["srrc".into()],
                extensions: vec!["rs".into()],
                patterns: vec!["forbidden".into()],
                exclude_patterns: Vec::new(),
                allowlist: Vec::new(),
            }],
            arch_rules: Vec::new(),
        };

        let error =
            validate_audit_config(&test_dir.0, &config).expect_err("missing audit roots must fail");
        assert!(error.to_string().contains("is missing"));
    }

    #[test]
    fn rule_root_cannot_escape_project() {
        let test_dir = TestDir::new("outside-root");
        let config = Config {
            version: AUDIT_CONFIG_VERSION,
            engine: configured_audit().engine,
            paths: PathsConfig::default(),
            hard_rules: vec![HardRule {
                name: "outside".into(),
                severity: "blocker".into(),
                paths: vec!["../outside".into()],
                extensions: vec!["rs".into()],
                patterns: vec!["forbidden".into()],
                exclude_patterns: Vec::new(),
                allowlist: Vec::new(),
            }],
            arch_rules: Vec::new(),
        };

        let error = validate_audit_config(&test_dir.0, &config)
            .expect_err("audit roots must stay inside project");
        assert!(error.to_string().contains("may not escape"));
    }

    #[test]
    fn invalid_rule_regex_returns_an_error() {
        let error = compile_regexes(&["(".to_string()]).expect_err("invalid regex must fail");
        assert!(error.to_string().contains("invalid audit regex"));
    }

    #[test]
    fn log_parser_keeps_the_error_trace() {
        let test_dir = TestDir::new("parse-logs");
        let input = test_dir.0.join("input.jsonl");
        let output = test_dir.0.join("output.json");
        fs::write(
            &input,
            concat!(
                "{\"level\":\"INFO\",\"trace_id\":\"failed\",\"fields\":{\"message\":\"start\"}}\n",
                "{\"level\":\"ERROR\",\"trace_id\":\"failed\",\"fields\":{\"error\":\"root cause\"}}\n",
                "{\"level\":\"INFO\",\"trace_id\":\"other\",\"fields\":{\"message\":\"later\"}}\n"
            ),
        )
        .expect("write log fixture");

        log_parser::extract_error_context(&input.to_string_lossy(), &output.to_string_lossy())
            .expect("parse logs");
        let parsed: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(output).expect("read output")).expect("output JSON");

        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|entry| entry["trace_id"] == "failed"));
    }

    #[test]
    fn log_parser_reads_trace_id_from_the_current_span() {
        let test_dir = TestDir::new("parse-span-logs");
        let input = test_dir.0.join("input.jsonl");
        let output = test_dir.0.join("output.json");
        fs::write(
            &input,
            concat!(
                "{\"level\":\"INFO\",\"span\":{\"trace_id\":\"span-trace\"},\"fields\":{\"message\":\"start\"}}\n",
                "{\"level\":\"ERROR\",\"span\":{\"trace_id\":\"span-trace\"},\"fields\":{\"error\":\"root cause\"}}\n"
            ),
        )
        .expect("write log fixture");

        log_parser::extract_error_context(&input.to_string_lossy(), &output.to_string_lossy())
            .expect("parse logs");
        let parsed: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(output).expect("read output")).expect("output JSON");

        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|entry| entry["trace_id"] == "span-trace"));
    }

    #[test]
    fn log_parser_keeps_the_error_in_a_long_trace() {
        let test_dir = TestDir::new("parse-long-logs");
        let input = test_dir.0.join("input.jsonl");
        let output = test_dir.0.join("output.json");
        let mut logs = String::new();
        for index in 0..35 {
            logs.push_str(&format!(
                "{{\"level\":\"INFO\",\"trace_id\":\"long-trace\",\"fields\":{{\"message\":\"before {index}\"}}}}\n"
            ));
        }
        logs.push_str(
            "{\"level\":\"ERROR\",\"trace_id\":\"long-trace\",\"fields\":{\"error\":\"retained root cause\"}}\n",
        );
        for index in 0..10 {
            logs.push_str(&format!(
                "{{\"level\":\"INFO\",\"trace_id\":\"long-trace\",\"fields\":{{\"message\":\"after {index}\"}}}}\n"
            ));
        }
        fs::write(&input, logs).expect("write log fixture");

        log_parser::extract_error_context(&input.to_string_lossy(), &output.to_string_lossy())
            .expect("parse logs");
        let parsed: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(output).expect("read output")).expect("output JSON");

        assert_eq!(parsed.len(), 30);
        assert!(parsed
            .iter()
            .any(|entry| entry["error"] == "retained root cause"));
    }
}
