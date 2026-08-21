import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";
import {
  checkTemplates,
  extractPlaceholders,
  renderTemplate,
  validateSql,
} from "./check-templates.mjs";

const createdRoots = [];

afterEach(() => {
  while (createdRoots.length > 0) {
    rmSync(createdRoots.pop(), { recursive: true, force: true });
  }
});

test("production template catalog passes the quality gate", () => {
  const result = checkTemplates();
  assert.equal(result.total, 8);
  assert.deepEqual(result.byLanguage, { typescript: 3, rust: 4, sql: 1 });
});

test("renderer requires an exact placeholder value set", () => {
  const source = "export interface {{ENTITY_NAME}} {}\n";
  assert.deepEqual(extractPlaceholders(source), new Set(["ENTITY_NAME"]));
  assert.equal(
    renderTemplate(source, { ENTITY_NAME: "StockQuote" }),
    "export interface StockQuote {}\n",
  );
  assert.throws(() => renderTemplate(source, {}), /缺少样例值: ENTITY_NAME/);
  assert.throws(
    () =>
      renderTemplate(source, { ENTITY_NAME: "StockQuote", UNUSED: "value" }),
    /未使用样例值: UNUSED/,
  );
});

test("SQL validator rejects broken quotes, parentheses and terminators", () => {
  assert.doesNotThrow(() =>
    validateSql("INSERT INTO demo (name) VALUES ('行情');\n"),
  );
  assert.throws(
    () => validateSql("INSERT INTO demo (name VALUES ('行情');\n"),
    /括号不平衡/,
  );
  assert.throws(
    () => validateSql("INSERT INTO demo VALUES ('行情);\n"),
    /未闭合/,
  );
  assert.throws(
    () => validateSql("INSERT INTO demo VALUES ('行情')\n"),
    /分号/,
  );
});

test("catalog rejects templates that are not registered", () => {
  const fixture = createFixture();
  writeFileSync(
    join(fixture.templates, "known.sql.tmpl"),
    "SELECT '{{VALUE}}';\n",
  );
  writeFileSync(join(fixture.templates, "rogue.sql.tmpl"), "SELECT 1;\n");
  writeManifest(fixture.manifest, [
    {
      file: "known.sql.tmpl",
      language: "sql",
      outputFile: "known.sql",
      values: { VALUE: "ok" },
    },
  ]);

  assert.throws(
    () =>
      checkTemplates({
        templateDir: fixture.templates,
        manifestPath: fixture.manifest,
      }),
    /未登记模板: rogue\.sql\.tmpl/,
  );
});

test("catalog rejects malformed rendered TypeScript", () => {
  const fixture = createFixture();
  writeFileSync(
    join(fixture.templates, "broken.ts.tmpl"),
    "export const {{NAME}} = ;\n",
  );
  writeManifest(fixture.manifest, [
    {
      file: "broken.ts.tmpl",
      language: "typescript",
      outputFile: "broken.ts",
      values: { NAME: "BROKEN" },
    },
  ]);

  assert.throws(
    () =>
      checkTemplates({
        templateDir: fixture.templates,
        manifestPath: fixture.manifest,
      }),
    /typescript 语法检查失败/,
  );
});

test("catalog rejects malformed rendered Rust", () => {
  const fixture = createFixture();
  writeFileSync(
    join(fixture.templates, "broken.rs.tmpl"),
    "pub fn {{NAME}}( {\n",
  );
  writeManifest(fixture.manifest, [
    {
      file: "broken.rs.tmpl",
      language: "rust",
      outputFile: "broken.rs",
      values: { NAME: "broken" },
    },
  ]);

  assert.throws(
    () =>
      checkTemplates({
        templateDir: fixture.templates,
        manifestPath: fixture.manifest,
      }),
    /Rust 模板未通过 rustfmt 解析\/格式检查/,
  );
});

function createFixture() {
  const root = mkdtempSync(join(tmpdir(), "arc-template-gate-test-"));
  createdRoots.push(root);
  const templates = join(root, "templates");
  mkdirSync(templates);
  return { templates, manifest: join(templates, "manifest.json") };
}

function writeManifest(path, templates) {
  writeFileSync(
    path,
    JSON.stringify(
      {
        schemaVersion: 1,
        forbiddenPatterns: [],
        templates,
      },
      null,
      2,
    ) + "\n",
  );
}
