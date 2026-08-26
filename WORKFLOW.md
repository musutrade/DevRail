---
version: devrail-v1
execution_mode: implementation
tools:
  allow:
    - read_file
    - search
    - apply_patch
    - cargo_flow
  network: false
  dangerous_commands_require_approval: true
quality_gates:
  - cargo-flow-scope
  - cargo-flow-verify
timeout_seconds: 3600
stall_timeout_seconds: 120
retry:
  max_attempts: 3
  base_delay_seconds: 1
  max_delay_seconds: 300
hooks:
  before_run:
    - cargo-flow-scope
  after_run:
    - cargo-flow-verify
notifications:
  events:
    - awaiting_approval
    - succeeded
    - failed
---

请完成任务「{{ task.title | trim }}」。

目标：{{ task.goal | trim }}

验收标准：{{ task.acceptance_criteria | default("以任务详情中的约束和质量门禁为准") }}

仓库：{{ repository.name | default("当前受控仓库") }}

所有操作必须遵守 DevRail 审批、脱敏、受控工作区和默认网络关闭策略。
