## Purpose

为代码扫描、依赖审查、容器扫描和定时供应链检查建立真实执行、身份固定、风险到期和完整触发的统一门禁，避免工作流以绿色状态掩盖未运行或不可复现的安全检查。

## ADDED Requirements

### Requirement: Security checks execute under fixed identities

供应链 workflow MUST 对第三方 Action 使用不可变 commit SHA，对生产基础镜像和运行时镜像使用不可变 digest 或记录并验证构建解析出的 digest。配置门禁 MUST 验证实际引用和执行条件，而不是仅匹配 action 名称或字符串。

#### Scenario: Mutable action reference is introduced

- **WHEN** workflow 使用可移动 tag 或分支引用第三方 Action
- **THEN** 供应链配置检查失败并阻断合并

#### Scenario: Image identity is reproducible

- **WHEN** 生产镜像构建使用固定 digest 或已记录且经校验的解析 digest
- **THEN** 同一构建输入可复现相同基础层身份，扫描结果能关联到该身份

### Requirement: Security scanning covers all relevant triggers

系统 MUST 在 pull request、默认分支 push 和 schedule 场景下执行适用的 CodeQL、依赖审查、RustSec、Trivy 和 SBOM 检查。周期扫描 MUST 不因仅比较最近一次提交的路径过滤而跳过对默认分支当前依赖快照的检查。部署 nginx 或其他影响运行镜像的文件变更 MUST 触发镜像扫描和 SBOM。

#### Scenario: Scheduled scan runs without source changes

- **WHEN** schedule 在默认分支运行且最近提交未修改业务源代码
- **THEN** 依赖和镜像安全检查仍读取当前默认分支快照并执行，不以路径无变化跳过

#### Scenario: Nginx deployment configuration changes

- **WHEN** `deployment/nginx` 下影响运行镜像的配置发生变化
- **THEN** 对应容器扫描和 SBOM job 被触发，并将变更纳入扫描输入

#### Scenario: Workflow condition disables a required check

- **WHEN** required security job 的执行条件使其在适用事件中被跳过
- **THEN** 配置门禁失败并阻断合并，除非该事件明确不适用且有可审计理由

### Requirement: Accepted advisories expire automatically

任何依赖漏洞 ignore MUST 关联真实依赖路径、责任人、补偿控制和明确的 UTC 到期时刻。到期后仍存在该 ignore 时，本地检查和 CI workflow MUST 失败；只有移除 ignore、替换依赖或重新提交带新证据和未来到期日的风险接受才能恢复通过。

#### Scenario: Advisory acceptance is before expiry

- **WHEN** 当前 UTC 时间早于风险接受的到期时刻且记录完整
- **THEN** 供应链检查允许该明确列出的 ignore，并输出其责任人、路径和到期信息

#### Scenario: Advisory acceptance reaches expiry

- **WHEN** 当前 UTC 时间等于或晚于风险接受的到期时刻且 ignore 仍存在
- **THEN** 本地与 CI 供应链检查失败，并阻断相关依赖、构建或发布 job

#### Scenario: Advisory is replaced or renewed

- **WHEN** ignore 被移除，或责任人以新证据和未来到期日重新接受风险
- **THEN** 检查按新的依赖状态或新的接受记录重新计算结果，不保留旧接受的隐式有效性
