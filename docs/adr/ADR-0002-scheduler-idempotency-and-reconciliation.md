# ADR-0002：调度器的稳定幂等与对账语义

- 状态：Accepted
- 日期：2026-08-26
- 决策人：DevRail 项目维护者
- 关联需求：[SY-ORCH-002](../symphony-devrail-requirements.md#53-orchestrator-调度循环p0)、[SY-ORCH-003](../symphony-devrail-requirements.md#53-orchestrator-调度循环p0)、[SY-RETRY-002](../symphony-devrail-requirements.md#54-重试stall-与恢复p0)、[SY-RECON-001](../symphony-devrail-requirements.md#58-reconciliation-与终态处理p0)

## 背景

多个 worker、进程重启、网关断流和 app-server 异常都可能让任务、run、claim 与实际子进程状态短暂不一致。仅依赖随机 claim 标识或单次 dispatch 结果，可能造成重复 run、重复通知或永久活动任务。

## 决策

1. 业务幂等键使用稳定的 `task_id + attempt`；claim UUID 只用于租约实例标识，不作为业务幂等依据。
2. 领取使用 PostgreSQL 原子事务、`FOR UPDATE SKIP LOCKED`、租约过期时间和续租；过期 claim 可被其他 worker 重新领取。
3. 每轮控制循环固定先执行 reconciliation，再 dispatch，最后执行终态回收和指标更新。
4. 调度使用独立的 Scheduler/System Actor，并为状态修正记录原因、来源和 trace。
5. 发现数据库、Supervisor 和 workspace 不一致时，按确定性策略恢复、重排队或失败；重复终态事件必须幂等。

## 取舍与后果

### 正面影响

- 多实例和重启场景下不会依赖内存状态，任务最终能恢复或明确失败。
- 稳定 attempt 语义便于关联 retry、continuation、通知、changeset 和修复 run。
- PostgreSQL 是现有可靠性前置条件，不增加 Redis 依赖。

### 代价与风险

- 需要迁移和集成测试验证租约、心跳、唯一约束及时钟边界。
- reconciliation 可能终止异常遗留的子进程，必须保存诊断上下文并告警。
- 退避和 stall 阈值需要按环境调优，不能用无限重试掩盖系统故障。

## 拒绝的方案

- 不使用随机 claim ID 作为任务级幂等键。
- 不通过 `session_id = 0` 伪造普通用户上下文。
- 不把浏览器 SSE 重连当作 Agent 重试，也不在 Handler 请求中执行调度循环。

## 关联实现

本 ADR 由 `symphony-orchestrator-reconciliation` OpenSpec change 实现。

实现证据：

- `devrail_runs.attempt`、System Actor、心跳、重试、父 run/turn 和 cleanup 字段由 `20260826030000_add_symphony_scheduler_reliability.sql` 添加；历史 run 回填和旧版 INSERT 兼容均有迁移演练。
- `claim_scheduler_tasks`、`scheduler_claim_is_current` 和 `reconcile_scheduler_state` 使用 PostgreSQL 行锁、租约与单事务状态修正。
- `HarnessSupervisor` 在释放旧进程控制通道和并发槽位后，使用持久化 thread/turn 恢复同一 run/attempt；无 thread 时进入下一 attempt，避免重复 Agent。
- System Actor 自动状态变更和 reconciliation 均保存原因、策略版本与 UUID trace；终态通知/outbox 使用稳定 source key。
- 真实 PostgreSQL 测试覆盖多 worker、租约过期、旧 token、priority aging、重复 run、取消、stale/restart、审计和通知幂等；受控假 app-server 覆盖 stall 与传输 EOF 恢复。

运行参数、指标、告警和回滚见 [Symphony Orchestrator 运行手册](../symphony-orchestrator-operations.md)。
