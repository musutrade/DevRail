import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";
import { upgradeProject } from "./upgrade-framework.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const createdRoots = [];

function git(root, args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
}

function write(root, path, contents) {
  const target = join(root, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
}

function initializeGit(root) {
  git(root, ["init", "-q"]);
  git(root, ["config", "user.name", "Test User"]);
  git(root, ["config", "user.email", "test@example.test"]);
}

function commit(root, message) {
  git(root, ["add", "."]);
  git(root, ["commit", "-qm", message]);
}

function createUpgradeFixture({ conflict = false, reclassify = false } = {}) {
  const sourceRoot = mkdtempSync(join(tmpdir(), "arc-framework-source-"));
  const targetRoot = mkdtempSync(join(tmpdir(), "arc-framework-target-"));
  createdRoots.push(sourceRoot, targetRoot);
  initializeGit(sourceRoot);

  const manifest = `${JSON.stringify(
    {
      schemaVersion: 1,
      framework: "arc-admin",
      managedRoots: [".arc-framework", "framework"],
      managedFiles: ["CHANGELOG.md", "FRAMEWORK_VERSION"],
    },
    null,
    2,
  )}\n`;
  write(sourceRoot, ".arc-framework/manifest.json", manifest);
  write(sourceRoot, "FRAMEWORK_VERSION", "v1.1.0\n");
  write(sourceRoot, "CHANGELOG.md", "## [v1.1.0]\n");
  write(
    sourceRoot,
    "framework/settings.txt",
    "name=arc-admin\ncolor=blue\nfooter=old\n",
  );
  write(sourceRoot, "framework/remove.txt", "remove me\n");
  commit(sourceRoot, "v1.1.0");
  git(sourceRoot, ["tag", "v1.1.0"]);

  initializeGit(targetRoot);
  write(targetRoot, ".arc-framework/manifest.json", manifest);
  write(targetRoot, "FRAMEWORK_VERSION", "v1.1.0\n");
  write(targetRoot, "CHANGELOG.md", "## [v1.1.0]\n");
  write(
    targetRoot,
    "framework/settings.txt",
    conflict
      ? "name=arc-admin\ncolor=blue\nfooter=business\n"
      : "name=stock-analysis\ncolor=blue\nfooter=old\n",
  );
  write(targetRoot, "framework/remove.txt", "remove me\n");
  write(
    targetRoot,
    ".arc-project.json",
    `${JSON.stringify(
      {
        schemaVersion: 1,
        framework: { name: "arc-admin", version: "v1.1.0" },
        project: { slug: "stock-analysis" },
      },
      null,
      2,
    )}\n`,
  );
  commit(targetRoot, "business baseline");

  write(sourceRoot, "FRAMEWORK_VERSION", "v1.2.0\n");
  write(sourceRoot, "CHANGELOG.md", "## [v1.2.0]\n\n## [v1.1.0]\n");
  if (reclassify) {
    write(
      sourceRoot,
      ".arc-framework/manifest.json",
      `${JSON.stringify(
        {
          schemaVersion: 1,
          framework: "arc-admin",
          managedRoots: [".arc-framework"],
          managedFiles: ["CHANGELOG.md", "FRAMEWORK_VERSION"],
        },
        null,
        2,
      )}\n`,
    );
  }
  write(
    sourceRoot,
    "framework/settings.txt",
    "name=arc-admin\ncolor=blue\nfooter=framework-v2\n",
  );
  unlinkSync(join(sourceRoot, "framework/remove.txt"));
  write(sourceRoot, "framework/new.txt", "new framework file\n");
  commit(sourceRoot, "v1.2.0");
  git(sourceRoot, ["tag", "v1.2.0"]);

  return { sourceRoot, targetRoot };
}

afterEach(() => {
  while (createdRoots.length > 0) {
    rmSync(createdRoots.pop(), { recursive: true, force: true });
  }
});

test("previews and applies a release while preserving non-conflicting business changes", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture();

  const preview = upgradeProject({
    sourceRoot,
    targetRoot,
    checkOnly: true,
    skipVerify: true,
  });
  assert.equal(preview.fromVersion, "v1.1.0");
  assert.equal(preview.toVersion, "v1.2.0");
  assert.equal(preview.merged, 1);
  assert.equal(
    readFileSync(join(targetRoot, "FRAMEWORK_VERSION"), "utf8"),
    "v1.1.0\n",
  );
  assert.equal(git(targetRoot, ["status", "--porcelain"]), "");

  const result = upgradeProject({
    sourceRoot,
    targetRoot,
    skipVerify: true,
    now: () => new Date("2026-08-08T08:00:00.000Z"),
  });
  assert.equal(result.merged, 1);
  assert.equal(
    readFileSync(join(targetRoot, "framework/settings.txt"), "utf8"),
    "name=stock-analysis\ncolor=blue\nfooter=framework-v2\n",
  );
  assert.equal(
    readFileSync(join(targetRoot, "framework/new.txt"), "utf8"),
    "new framework file\n",
  );
  assert.equal(
    git(targetRoot, ["status", "--short", "framework/remove.txt"]),
    "D framework/remove.txt",
  );
  const metadata = JSON.parse(
    readFileSync(join(targetRoot, ".arc-project.json"), "utf8"),
  );
  assert.equal(metadata.schemaVersion, 2);
  assert.equal(metadata.framework.initializedVersion, "v1.1.0");
  assert.equal(metadata.framework.version, "v1.2.0");
  assert.equal(metadata.framework.upgradedAt, "2026-08-08T08:00:00.000Z");
});

test("reports same-line conflicts before writing any file", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture({ conflict: true });

  assert.throws(
    () =>
      upgradeProject({
        sourceRoot,
        targetRoot,
        skipVerify: true,
      }),
    /framework\/settings\.txt.*冲突/s,
  );
  assert.equal(
    readFileSync(join(targetRoot, "FRAMEWORK_VERSION"), "utf8"),
    "v1.1.0\n",
  );
  assert.equal(git(targetRoot, ["status", "--porcelain"]), "");
});

test("allows an explicitly reviewed conflict to keep the business version", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture({ conflict: true });

  const result = upgradeProject({
    sourceRoot,
    targetRoot,
    acceptPaths: ["framework/settings.txt"],
    skipVerify: true,
  });
  assert.deepEqual(result.accepted, ["framework/settings.txt"]);
  assert.match(
    readFileSync(join(targetRoot, "framework/settings.txt"), "utf8"),
    /footer=business/,
  );
  assert.equal(
    readFileSync(join(targetRoot, "FRAMEWORK_VERSION"), "utf8"),
    "v1.2.0\n",
  );
});

test("preserves files that still exist after leaving the managed manifest", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture({ reclassify: true });

  upgradeProject({ sourceRoot, targetRoot, skipVerify: true });

  assert.equal(
    readFileSync(join(targetRoot, "framework/settings.txt"), "utf8"),
    "name=stock-analysis\ncolor=blue\nfooter=old\n",
  );
  assert.equal(existsSync(join(targetRoot, "framework/new.txt")), false);
  assert.equal(
    readFileSync(join(targetRoot, "FRAMEWORK_VERSION"), "utf8"),
    "v1.2.0\n",
  );
});

test("rejects dirty targets and framework downgrades", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture();
  write(targetRoot, "business.txt", "uncommitted\n");
  assert.throws(
    () => upgradeProject({ sourceRoot, targetRoot, skipVerify: true }),
    /必须保持干净/,
  );

  rmSync(join(targetRoot, "business.txt"));
  const metadataPath = join(targetRoot, ".arc-project.json");
  const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
  metadata.framework.version = "v9.0.0";
  writeFileSync(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`);
  commit(targetRoot, "future framework");
  assert.throws(
    () => upgradeProject({ sourceRoot, targetRoot, skipVerify: true }),
    /拒绝.*降级/,
  );
});

test("requires the template checkout to point exactly at the target release tag", () => {
  const { sourceRoot, targetRoot } = createUpgradeFixture();
  write(sourceRoot, "after-release.txt", "not released\n");
  commit(sourceRoot, "post-release work");

  assert.throws(
    () => upgradeProject({ sourceRoot, targetRoot, skipVerify: true }),
    /必须检出目标标签 v1\.2\.0/,
  );
  assert.equal(git(targetRoot, ["status", "--porcelain"]), "");
});

test("shell entrypoint has valid syntax and Chinese help", () => {
  const entrypoint = join(SCRIPT_DIR, "upgrade-framework.sh");
  execFileSync("bash", ["-n", entrypoint]);
  const help = execFileSync("bash", [entrypoint, "--help"], {
    encoding: "utf8",
  });
  assert.match(help, /升级的业务仓库/);
  assert.match(help, /--accept/);
});
