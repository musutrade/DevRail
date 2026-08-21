#!/usr/bin/env node

import {
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function run(program, args, cwd) {
  const result = spawnSync(program, args, { cwd, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(
      `${program} ${args.join(" ")} 执行失败\n${result.stdout}${result.stderr}`,
    );
  }
}

function filesUnder(root, base = root) {
  const files = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) {
      files.push(...filesUnder(path, base));
    } else {
      files.push(relative(base, path));
    }
  }
  return files.sort();
}

function assertFileEqual(expected, actual, label) {
  if (readFileSync(expected, "utf8") !== readFileSync(actual, "utf8")) {
    throw new Error(`${label} 已过期，请执行 npm run generate:api:all`);
  }
}

function assertDirectoryEqual(expected, actual) {
  const expectedFiles = filesUnder(expected);
  const actualFiles = filesUnder(actual);
  if (JSON.stringify(expectedFiles) !== JSON.stringify(actualFiles)) {
    throw new Error("Angular API 生成文件清单已过期，请执行 npm run generate:api:all");
  }
  for (const path of expectedFiles) {
    assertFileEqual(join(expected, path), join(actual, path), `Angular API 文件 ${path}`);
  }
}

const temporary = mkdtempSync(join(tmpdir(), "arc-admin-openapi-"));
try {
  const generatedOpenApi = join(temporary, "openapi.json");
  const generatedAngular = join(temporary, "api");
  run(
    "cargo",
    [
      "run",
      "--quiet",
      "--manifest-path",
      "backend/Cargo.toml",
      "--bin",
      "export_openapi",
      "--",
      generatedOpenApi,
    ],
    ROOT,
  );
  assertFileEqual(
    join(ROOT, "docs/openapi.json"),
    generatedOpenApi,
    "OpenAPI 文档",
  );
  run(
    join(ROOT, "frontend/node_modules/.bin/ng-openapi-gen"),
    [
      "--config",
      "ng-openapi-gen.json",
      "--input",
      generatedOpenApi,
      "--output",
      generatedAngular,
      "--silent",
      "true",
    ],
    join(ROOT, "frontend"),
  );
  assertDirectoryEqual(join(ROOT, "frontend/src/app/generated/api"), generatedAngular);
  console.log("API 生成门禁通过: Rust OpenAPI 与 Angular Client 均为最新");
} catch (error) {
  console.error(`API 生成门禁失败: ${error.message}`);
  process.exitCode = 1;
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
