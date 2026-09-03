# 供应链安全

项目将依赖、源码和容器镜像安全检查放在 GitHub Actions 中执行，不要求开发机长期安装扫描器。

## 自动检查

- `security.yml` 每次 PR、`main` 分支推送及每周定时执行 RustSec `cargo audit`、`cargo deny`、Trivy 镜像扫描。
- `cargo deny` 根据根目录 `deny.toml` 检查漏洞公告、许可证、重复依赖和依赖来源。新增许可证例外时必须注明业务理由并接受评审。
- 后端、前端生产镜像均阻断已修复的高危或严重漏洞；未修复漏洞保留在报告中，但不阻断，以免项目因上游尚无补丁而永久失效。
- 两个生产镜像分别生成 SPDX JSON SBOM，并作为 Actions 产物保留 30 天。
- `codeql.yml` 对 Rust 与 TypeScript 执行扩展安全查询，适用于当前 INTERNAL 仓库；是否可用取决于组织已启用的 GitHub Advanced Security 配额。

## 本地核验

已安装工具时可从仓库根执行：

```bash
cargo audit --file backend/Cargo.lock
(cd backend && cargo deny check advisories bans licenses sources)
node scripts/check-supply-chain.mjs
```

`cargo flow verify --components workflow` 会执行最后一项静态配置门禁。漏洞库依赖网络且会随时间变化，因此在线漏洞结果由定时 Actions 负责，提交前全量门禁不伪造离线安全结论。当前 `RUSTSEC-2023-0071` 的临时风险接受及到期日见 [风险接受记录](security/rustsec-2023-0071-risk-acceptance-2026-09.md)。
