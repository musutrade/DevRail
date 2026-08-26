## 1. 数据模型与迁移

- [x] 1.1 盘点现有 task、run、claim、事件和审计字段，形成迁移字段映射并在设计评审记录中确认无重复语义；验证 `cargo flow scope` 只将预期 backend/frontend/workflow 文件纳入范围
- [x] 1.2 添加 attempt、Scheduler/System Actor、租约续期、心跳和重试元数据的向后兼容迁移；验证迁移可在空数据库和包含历史 run 的测试数据库上成功执行
- [x] 1.3 添加单任务单活动 run、稳定幂等键和当前受控 workspace/claim 模型所需约束；验证重复插入测试返回确定性冲突且不破坏历史审计

## 2. 领取与幂等

- [x] 2.1 在 Repository 中实现带状态、活动 run 和租约条件的原子任务领取；验证两个并发 worker 集成测试只产生一个有效 claim
- [x] 2.2 将业务幂等键固定为 `task_id + attempt`，把随机 claim ID 限定为租约实例标识；验证重复 dispatch 测试复用已有 run 且不重复发送通知
- [x] 2.3 实现 claim 续租、过期释放和旧 worker 写入拒绝；验证停止续租后其他 worker 可领取，旧 worker 的状态更新被拒绝
- [x] 2.4 接入明确的 Scheduler/System Actor 和最小权限上下文；验证自动状态变更的审计记录包含 actor、原因、策略版本和 trace

## 3. Reconciliation 控制循环

- [x] 3.1 将 worker 循环整理为 `reconcile → claim/dispatch → reap/metrics`，支持优雅停止；验证单元测试和 worker 启停测试确认执行顺序及退出无阻塞
- [x] 3.2 对账数据库 run、claim、Supervisor 快照、子进程和 workspace 状态；验证“数据库 active 但进程不存在”“进程已退出但数据库未更新”场景能恢复或明确失败
- [x] 3.3 处理任务取消、环境失效和策略变化对待启动 run 的传播；验证取消竞态测试不会启动 Agent，并释放 claim
- [x] 3.4 实现终态回收、子进程清理和幂等状态更新；验证重复退出/超时事件只生成一个终态、一组通知和一次清理结果

## 4. 重试、stall 与传输恢复

- [x] 4.1 实现可重试/不可重试错误分类及带抖动的指数退避；验证退避单元测试覆盖初始延迟、最大延迟、最大 attempt 和随机抖动边界
- [x] 4.2 实现无心跳、无事件、进程退出和租约失效的 stall 检测；验证 stall 集成测试按策略中断、重新排队或失败，并保留脱敏原因
- [x] 4.3 区分 Agent/app-server 传输断流与浏览器 SSE 断开；验证传输恢复不创建重复 Agent，SSE 重连只补拉事件且不改变任务事实
- [x] 4.4 实现进程重启后的活动 run 扫描和恢复/失败决策；验证重启演练不存在无限期 active run，且不可恢复 run 触发通知

## 5. 观测、API 与前端状态

- [x] 5.1 增加队列深度、派发延迟、claim 冲突、重试、stall、活动 run 和 reconciliation 指标；验证指标名称、标签基数和脱敏规则测试通过
- [x] 5.2 在 run/task 查询响应中暴露当前 attempt、排队原因、下一次重试时间、stall/失败原因和恢复建议；验证 OpenAPI 契约测试与生成 Angular client 无漂移
- [x] 5.3 更新 Angular 任务详情和运行详情状态展示，覆盖排队、恢复、重试和不可恢复失败；验证 Vitest 组件测试及权限显隐测试通过

## 6. 集成验收与文档

- [x] 6.1 补齐真实 PostgreSQL 多 worker 竞争、claim 过期、重启、断流、超时、取消和终态幂等集成测试；验证相关 Rust 测试在隔离 `TEST_DATABASE_URL` 下通过
- [x] 6.2 执行 secret scan、架构审计、Rust/Angular 测试和 `cargo flow verify --all`；验证完整门禁输出 `TEST_SUMMARY: PASS`
- [x] 6.3 更新 `docs/devrail-implementation-status.md`、专项需求证据矩阵和运行手册，关联 ADR-0001、ADR-0002 及本 change；验证文档链接、需求 ID 和测试命令可复查
- [x] 6.4 在 PR 描述中列出需求 ID、ADR、OpenSpec change、迁移、测试、风险和回滚方案，并监控 CI/供应链/CodeQL；验证所有 required checks 成功后再归档 change
