# 框架版本与派生项目升级

arc-admin 使用语义化版本和不可移动的 Git 标签发布。业务仓库通过新版本模板中的升级命令，把旧标签、业务仓库当前内容和新标签执行三方合并，从而保留业务改动并获得框架修复。

## 边界

`.arc-framework/manifest.json` 定义框架管理范围。升级器只处理旧版或新版模板 Git 树中真实存在、并且命中该清单的文件。

- 框架已有文件：参与三方合并；
- 业务仓库独自新增的业务文件：不会被扫描或删除；
- 框架与业务新增同一路径：作为冲突处理；
- `backend/.env`、`observability/.env`、`deployment/.env.production` 等本地秘密：不在 Git 树中，不参与升级；
- `.arc-project.json`：由升级器结构化更新版本元数据，不使用模板内容覆盖。

即使业务文件位于 `backend/src`、`frontend/src` 或 `docs` 等管理目录，只要模板的旧、新版本都没有同一路径，升级器就不会触碰它。不要把业务实现写入现有框架文件；确需修改时，三方合并会尽量保留，冲突则要求人工判断。

## 升级业务仓库

对于已经存在、但尚未生成 `.arc-project.json` 的业务仓库，先在框架仓库执行一次登记。登记命令只写入版本元数据，不会覆盖业务代码、README、运行时配置或本地环境文件：

```bash
../arc-admin-framework/scripts/adopt-project.sh --target "$PWD" --slug devflow --title DevFlow --short-name "研发流程 Agent 平台" --database devflow --permission-prefix devflow
```

登记前必须保证业务仓库工作区干净。`Aisix-Panel` 使用自己的 slug、数据库名和权限前缀；不要用初始化脚本代替登记，因为初始化脚本会生成环境文件并改写项目头部。

先取得包含完整标签历史的新版本模板。以下示例把业务项目从 `v2.3.0` 升级到假设的新版本 `v2.4.0`：

```bash
gh repo clone higoalespn/arc-admin ../arc-admin-framework
git -C ../arc-admin-framework fetch --tags
git -C ../arc-admin-framework checkout v2.4.0
```

业务仓库必须已经执行过项目初始化，且所有改动均已提交。进入业务仓库后先预检：

```bash
../arc-admin-framework/scripts/upgrade-framework.sh --check
```

预检通过后执行升级：

```bash
../arc-admin-framework/scripts/upgrade-framework.sh
```

升级命令默认执行：

1. 校验业务仓库干净且禁止降级；
2. 校验当前版本和目标版本标签均存在；
3. 读取两个版本的框架管理清单；
4. 对框架变更执行三方合并，并在写入前汇总所有冲突；
5. 更新 `FRAMEWORK_VERSION` 和 `.arc-project.json`；
6. 执行 `cargo flow doctor`；
7. 执行 `cargo flow verify --all`。

命令不会提交或推送。验证通过后检查 `git diff`，再由开发者提交升级结果。

## 处理冲突

检测到冲突时，升级器不会写入任何文件。先在模板仓库查看版本差异：

```bash
git -C ../arc-admin-framework diff v2.3.0 v2.4.0 -- <冲突文件>
```

在业务仓库人工合并该文件，执行必要测试并提交。确认已经吸收所需框架改动后，显式保留该文件并继续升级：

```bash
../arc-admin-framework/scripts/upgrade-framework.sh \
  --accept backend/src/lib.rs \
  --accept frontend/src/app/app.routes.ts
```

`--accept` 表示该文件完全采用业务仓库当前内容，升级器不会再合并模板版本。它不能用于 `FRAMEWORK_VERSION` 或框架清单。不得为了消除冲突而未经审查批量使用该选项。

如果完整验证因环境暂时不可用，可显式使用 `--skip-verify`，但提交前仍必须运行：

```bash
cargo flow verify --all
```

## 发布新框架版本

`main` 应始终对应最新正式发布版本。下一版本开发在独立分支完成，不要让 GitHub Template 默认分支长期处于无法升级的中间状态。

发布步骤：

1. 确认新增框架文件命中 `.arc-framework/manifest.json`；
2. 将 `FRAMEWORK_VERSION` 更新为新的 `vX.Y.Z`；
3. 在 `CHANGELOG.md` 增加同名版本、日期、变更和人工升级说明；
4. 新增数据库迁移时使用 UTC 时间戳版本，禁止修改旧迁移；
5. 增加或更新升级测试；
6. 执行 `cargo flow verify --all`；
7. 提交发布内容后创建同名不可移动标签；
8. 人工推送提交和标签。

示例：

```bash
git tag -a v2.4.0 -m "arc-admin v2.4.0"
git push origin main
git push origin v2.4.0
```

标签一旦被派生项目用作三方合并基线，不得删除、覆盖或重新指向其他提交。发布后应使用一个临时业务仓库实际演练 `--check` 和完整升级。

## 版本元数据

初始化后的 `.arc-project.json` 保存：

```json
{
  "schemaVersion": 2,
  "framework": {
    "name": "arc-admin",
    "version": "v2.3.0",
    "initializedVersion": "v2.3.0"
  }
}
```

升级后保留 `initializedVersion`，更新 `version`，并增加 `upgradedAt`。业务代码可以读取项目配置，但不得把框架版本当作业务功能开关。
