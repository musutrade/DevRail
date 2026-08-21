import { execFileSync } from "node:child_process";
import {
  readFileSync,
  readdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirname(SCRIPT_PATH), "..");
const DEFAULT_TEMPLATE_DIR = join(
  REPO_ROOT,
  "codex-audit-pipeline",
  ".codex",
  "templates",
);
const DEFAULT_MANIFEST_PATH = join(DEFAULT_TEMPLATE_DIR, "manifest.json");
const PLACEHOLDER_PATTERN = /{{([A-Za-z][A-Za-z0-9_]*)}}/g;
const PLACEHOLDER_NAME_PATTERN = /^[A-Za-z][A-Za-z0-9_]*$/;
const CONFLICT_MARKER_PATTERN = /(^|\n)(<{7}|={7}|>{7})(?:\s|$)/;
const SUPPORTED_LANGUAGES = new Set(["typescript", "rust", "sql"]);

export function extractPlaceholders(source) {
  return new Set(
    Array.from(source.matchAll(PLACEHOLDER_PATTERN), (match) => match[1]),
  );
}

export function renderTemplate(source, values) {
  const placeholders = extractPlaceholders(source);
  const valueNames = new Set(Object.keys(values));
  const missing = difference(placeholders, valueNames);
  const unused = difference(valueNames, placeholders);
  if (missing.length > 0 || unused.length > 0) {
    throw new Error(
      "占位符不匹配" +
        formatDifference("缺少样例值", missing) +
        formatDifference("未使用样例值", unused),
    );
  }
  for (const [name, value] of Object.entries(values)) {
    if (typeof value !== "string" || value.length === 0) {
      throw new Error("占位符 " + name + " 的样例值必须是非空字符串");
    }
    if (value.includes("{{") || value.includes("}}")) {
      throw new Error("占位符 " + name + " 的样例值不能包含模板标记");
    }
  }
  return source.replace(PLACEHOLDER_PATTERN, (_match, name) => values[name]);
}

export function validateSql(source) {
  let parentheses = 0;
  let singleQuote = false;
  let doubleQuote = false;
  let lineComment = false;
  let blockComment = false;

  for (let index = 0; index < source.length; index += 1) {
    const current = source[index];
    const next = source[index + 1];

    if (lineComment) {
      if (current === "\n") {
        lineComment = false;
      }
      continue;
    }
    if (blockComment) {
      if (current === "*" && next === "/") {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (singleQuote) {
      if (current === "'" && next === "'") {
        index += 1;
      } else if (current === "'") {
        singleQuote = false;
      }
      continue;
    }
    if (doubleQuote) {
      if (current === '"' && next === '"') {
        index += 1;
      } else if (current === '"') {
        doubleQuote = false;
      }
      continue;
    }
    if (current === "-" && next === "-") {
      lineComment = true;
      index += 1;
    } else if (current === "/" && next === "*") {
      blockComment = true;
      index += 1;
    } else if (current === "'") {
      singleQuote = true;
    } else if (current === '"') {
      doubleQuote = true;
    } else if (current === "(") {
      parentheses += 1;
    } else if (current === ")") {
      parentheses -= 1;
      if (parentheses < 0) {
        throw new Error("存在多余的右括号");
      }
    }
  }

  if (singleQuote || doubleQuote) {
    throw new Error("存在未闭合的 SQL 引号");
  }
  if (blockComment) {
    throw new Error("存在未闭合的 SQL 块注释");
  }
  if (parentheses !== 0) {
    throw new Error("SQL 括号不平衡");
  }
  const executable = source.replace(/--.*$/gm, "").trim();
  if (!executable.endsWith(";")) {
    throw new Error("SQL 模板必须以分号结束");
  }
}

export function validateTypescript(source, fileName, typescript) {
  const result = typescript.transpileModule(source, {
    compilerOptions: {
      target: typescript.ScriptTarget.ES2022,
      module: typescript.ModuleKind.ESNext,
      experimentalDecorators: true,
    },
    fileName,
    reportDiagnostics: true,
  });
  const errors = (result.diagnostics ?? []).filter(
    (diagnostic) => diagnostic.category === typescript.DiagnosticCategory.Error,
  );
  if (errors.length > 0) {
    const messages = errors.map((diagnostic) =>
      typescript.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
    );
    throw new Error(messages.join("; "));
  }
}

export function checkTemplates(options = {}) {
  const repoRoot = options.repoRoot ?? REPO_ROOT;
  const templateDir = options.templateDir ?? DEFAULT_TEMPLATE_DIR;
  const manifestPath = options.manifestPath ?? DEFAULT_MANIFEST_PATH;
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const errors = [];

  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.templates)) {
    throw new Error("模板清单必须使用 schemaVersion 1 并提供 templates 数组");
  }

  const actualFiles = readdirSync(templateDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".tmpl"))
    .map((entry) => entry.name)
    .sort();
  const registeredFiles = manifest.templates.map((entry) => entry.file);
  const duplicateFiles = duplicates(registeredFiles);
  if (duplicateFiles.length > 0) {
    errors.push("模板清单存在重复文件: " + duplicateFiles.join(", "));
  }
  errors.push(
    ...coverageErrors(
      actualFiles,
      registeredFiles,
      "未登记模板",
      "清单引用但文件不存在",
    ),
  );

  const forbiddenPatterns = compilePatterns(
    manifest.forbiddenPatterns ?? [],
    "全局 forbiddenPatterns",
    errors,
  );
  const renderedRust = [];
  let typescript;

  for (const entry of manifest.templates) {
    const label = typeof entry.file === "string" ? entry.file : "<未知模板>";
    if (
      typeof entry.file !== "string" ||
      basename(entry.file) !== entry.file ||
      !entry.file.endsWith(".tmpl")
    ) {
      errors.push(label + ": file 必须是模板目录下的 .tmpl 文件名");
      continue;
    }
    if (!actualFiles.includes(entry.file)) {
      continue;
    }
    if (!SUPPORTED_LANGUAGES.has(entry.language)) {
      errors.push(label + ": 不支持的 language " + String(entry.language));
      continue;
    }
    if (
      typeof entry.outputFile !== "string" ||
      basename(entry.outputFile) !== entry.outputFile
    ) {
      errors.push(label + ": outputFile 必须是无目录的文件名");
      continue;
    }
    const values = entry.values;
    if (
      values === null ||
      typeof values !== "object" ||
      Array.isArray(values)
    ) {
      errors.push(label + ": values 必须是对象");
      continue;
    }
    for (const name of Object.keys(values)) {
      if (!PLACEHOLDER_NAME_PATTERN.test(name)) {
        errors.push(label + ": 非法占位符名称 " + name);
      }
    }

    const source = readFileSync(join(templateDir, entry.file), "utf8");
    if (!source.endsWith("\n")) {
      errors.push(label + ": 文件末尾必须保留换行");
    }
    if (source.includes("\r")) {
      errors.push(label + ": 只能使用 LF 换行");
    }
    let rendered;
    try {
      rendered = renderTemplate(source, values);
    } catch (error) {
      errors.push(label + ": " + error.message);
      continue;
    }
    if (rendered.includes("{{") || rendered.includes("}}")) {
      errors.push(label + ": 渲染后仍有模板标记");
    }
    if (CONFLICT_MARKER_PATTERN.test(rendered)) {
      errors.push(label + ": 包含 Git 冲突标记");
    }
    for (const pattern of forbiddenPatterns) {
      if (pattern.test(rendered)) {
        errors.push(label + ": 命中禁止模式 " + pattern.source);
      }
    }
    const requiredPatterns = compilePatterns(
      entry.requiredPatterns ?? [],
      label + " requiredPatterns",
      errors,
    );
    for (const pattern of requiredPatterns) {
      if (!pattern.test(rendered)) {
        errors.push(label + ": 缺少必要模式 " + pattern.source);
      }
    }

    try {
      if (entry.language === "typescript") {
        typescript ??= loadTypescript(repoRoot);
        validateTypescript(rendered, entry.outputFile, typescript);
      } else if (entry.language === "sql") {
        validateSql(rendered);
      } else {
        renderedRust.push({ fileName: entry.outputFile, source: rendered });
      }
    } catch (error) {
      errors.push(
        label + ": " + entry.language + " 语法检查失败: " + error.message,
      );
    }
  }

  if (renderedRust.length > 0) {
    validateRust(renderedRust, options.rustfmtPath ?? "rustfmt", errors);
  }
  if (errors.length > 0) {
    throw new Error("模板质量门禁失败:\n- " + errors.join("\n- "));
  }

  const byLanguage = Object.fromEntries(
    Array.from(SUPPORTED_LANGUAGES, (language) => [
      language,
      manifest.templates.filter((entry) => entry.language === language).length,
    ]),
  );
  return { total: manifest.templates.length, byLanguage };
}

function loadTypescript(repoRoot) {
  const requireFromFrontend = createRequire(
    join(repoRoot, "frontend", "package.json"),
  );
  return requireFromFrontend("typescript");
}

function validateRust(files, rustfmtPath, errors) {
  const directory = mkdtempSync(join(tmpdir(), "arc-template-check-"));
  try {
    const paths = files.map(({ fileName, source }) => {
      const path = join(directory, fileName);
      writeFileSync(path, source);
      return path;
    });
    try {
      execFileSync(rustfmtPath, ["--edition", "2021", "--check", ...paths], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      const detail = [error.stdout, error.stderr]
        .filter(Boolean)
        .join("\n")
        .trim();
      errors.push(
        "Rust 模板未通过 rustfmt 解析/格式检查" +
          (detail ? ":\n" + detail : ""),
      );
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function compilePatterns(patterns, label, errors) {
  const compiled = [];
  for (const pattern of patterns) {
    try {
      compiled.push(new RegExp(pattern, "m"));
    } catch (error) {
      errors.push(
        label + " 包含无效正则 " + String(pattern) + ": " + error.message,
      );
    }
  }
  return compiled;
}

function coverageErrors(actual, registered, missingLabel, staleLabel) {
  const errors = [];
  const missing = difference(new Set(actual), new Set(registered));
  const stale = difference(new Set(registered), new Set(actual));
  if (missing.length > 0) {
    errors.push(missingLabel + ": " + missing.join(", "));
  }
  if (stale.length > 0) {
    errors.push(staleLabel + ": " + stale.join(", "));
  }
  return errors;
}

function difference(left, right) {
  return Array.from(left)
    .filter((item) => !right.has(item))
    .sort();
}

function duplicates(items) {
  const seen = new Set();
  const duplicatesFound = new Set();
  for (const item of items) {
    if (seen.has(item)) {
      duplicatesFound.add(item);
    }
    seen.add(item);
  }
  return Array.from(duplicatesFound).sort();
}

function formatDifference(label, values) {
  return values.length > 0 ? "；" + label + ": " + values.join(", ") : "";
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT_PATH) {
  try {
    const result = checkTemplates();
    console.log(
      "模板质量门禁通过: " +
        result.total +
        " 个模板 (TypeScript " +
        result.byLanguage.typescript +
        ", Rust " +
        result.byLanguage.rust +
        ", SQL " +
        result.byLanguage.sql +
        ")",
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
