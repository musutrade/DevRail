## 1. 基线、架构决策与数据模型

- [x] 1.1 执行 `cargo flow scope`，读取 `review_context.json` 及 Repository、Service、Handler、权限、Angular Model/Service 模板，形成明确变更文件清单；验证 scope 只包含预期 backend、frontend、workflow 范围
- [x] 1.2 新增 `ADR-0005-continuation-turn-lifecycle.md`，固化同 thread 新 turn/new child run、来源终态不可变、四类运行语义分离、handoff 先于 cleanup 和取消线性化点；验证 ADR 链接 proposal、specs 与迁移计划且文档链接检查通过
- [x] 1.3 新增 additive PostgreSQL migration，创建 `devrail_continuation_requests`、`devrail_run_handoffs`、数据范围字段、状态/claim/证据字段、唯一约束和索引，并扩展 task 状态与 run 谱系；验证空库及含历史 task/run 的一次性 PostgreSQL 正向迁移均成功
- [x] 1.4 按模板扩展 Rust 领域模型、状态枚举、workflow/task snapshot continuation 策略、请求/响应 DTO 与低基数错误码；验证默认关闭、默认限额、序列化往返、非法枚举和旧记录兼容测试通过
- [x] 1.5 增加 continuation read/create/cancel 权限及业务权限种子，确保新表包含组织、部门、所有者和项目边界；验证权限矩阵、迁移种子幂等和跨组织不可见测试通过

## 2. Repository 与事务事实

- [x] 2.1 按 Repository 模板实现 continuation 幂等创建、详情、分页列表和状态读取，所有 SQL 强制数据范围并以稳定幂等键返回原结果；通过同范围、跨组织、重复用户请求、重复 gate/review 证据和分页 PostgreSQL 测试
- [x] 2.2 实现 `pending/claimed` 的 `SKIP LOCKED` 领取、续租/过期释放、退避和 claim token 条件更新；通过多 worker 竞争、旧 worker 写入拒绝、claim 过期和进程重放 PostgreSQL 测试
- [x] 2.3 扩展 TaskTracker Repository，在单一事务中完成 `continuation_pending`、`running`、请求前终态恢复、child 终态投影和不可变状态历史；通过版本冲突、活动 run、取消/拒绝和重复终态测试
- [x] 2.4 扩展 run Repository，按 continuation request 唯一创建或复用 child run，保存 run kind、父 run/turn、序号和同 thread 身份；验证并发创建最多产生一个 run 且来源 run 状态和终态时间未变化
- [x] 2.5 实现 run handoff 的范围查询和按来源 run 唯一写入，保存固定提交、快照/changeset 摘要和证据状态；验证重复终态幂等、摘要不匹配、历史缺失和敏感字段拒绝测试通过
- [x] 2.6 将请求创建/取消、task 历史、领域事件、审计和 transactional outbox 组合为 Repository 事务边界；验证故障注入会整体回滚且重复操作不产生重复事件或通知事实

## 3. Service、Handler 与触发入口

- [x] 3.1 按 Service 模板实现授权用户追加上下文流程，校验来源 run 终态、任务状态、活动 run、UTF-8 16 KiB 限制、链深/次数和固化策略，并在持久化前规范化与脱敏；通过允许、超限、秘密输入、策略关闭和历史无 handoff 测试
- [x] 3.2 为质量门禁和审查要求修改实现仅供受信任后端调用的触发入口，校验 gate result 或 review event/thread、changeset 摘要和证据新鲜度；通过伪造前端触发、重复 webhook、过期证据和摘要漂移测试
- [x] 3.3 实现 continuation 查询、谱系列表和启动前取消服务，使用派发状态作为取消线性化边界；通过 pending/claimed 取消、已派发冲突、重复取消和跨范围访问测试
- [x] 3.4 按 Handler 模板增加创建、详情/列表和取消路由，接入 continuation 权限并统一映射未找到、无权限、策略拒绝、版本冲突和暂时不可用错误；通过路由与鉴权集成测试
- [x] 3.5 定义 continuation 创建、领取、派发、取消、拒绝和终态事件及 outbox payload，确保只包含通知 ID、事件类型、脱敏摘要和受控深链接；通过 secret、完整上下文、绝对路径和重复副作用扫描测试

## 4. Scheduler、Supervisor 与 Codex turn

- [x] 4.1 在 Scheduler reconciliation 的普通 queued 派发前处理 continuation claim，重新校验任务、来源 run、策略、容量和活动 run；通过优先顺序、临时退避、确定性拒绝和 queued claim 隔离测试
- [x] 4.2 实现 workspace 准备后的派发事务，原子创建/复用 child run、绑定 workspace、标记请求已派发并投影 task 为运行中；通过数据库提交前后故障注入验证重启不会重复创建 run 或启动 Agent
- [x] 4.3 扩展 Harness Supervisor/app-server adapter，以来源 `codex_thread_id` 恢复持久 thread 后启动新 turn，保存新的 turn ID 和稳定 start key；通过受控假 app-server 验证同 thread/new turn、一次启动和来源 turn 不变
- [x] 4.4 保持传输恢复、child run retry、continuation 和 follow-up task 的独立分支与幂等键；通过 EOF/网关断流、可重试失败、Agent follow-up 和再次 continuation 测试验证谱系不混用
- [x] 4.5 实现 claim 丢失、已存在 child run、启动前取消和取消/派发竞态的 reconciliation；验证 worker 重启后请求不会永久停在 claimed，也不会留下未关联进程或 workspace
- [x] 4.6 将 child run 终态接入请求完成、TaskTracker 投影、hook/cleanup、审计、outbox 和依赖传播，所有副作用按稳定键幂等；通过重复退出、超时和 webhook 终态事件测试

## 5. Workspace handoff 与重建

- [x] 5.1 在来源 run 终态 cleanup 前固化并校验 handoff，关联任务/workflow/environment 快照、仓库、固定提交和 changeset；验证 handoff 成功后才进入 cleanup，失败仅阻止 continuation 而不改写来源 run 结论
- [x] 5.2 扩展 Workspace Manager，从 handoff 的固定提交与 changeset 摘要创建新的隔离 workspace，校验受控根目录、仓库身份和工作树完整性；通过来源目录已删除后的重建、摘要不匹配和符号链接越界测试
- [x] 5.3 禁止复用 `cleanup_failed`、已清理或活动 workspace 路径，并分离来源与 child 的 hook/cleanup 幂等键；通过来源 cleanup 重试与 child 创建/清理并发测试
- [x] 5.4 在 Agent 启动前取消或 workspace 准备失败时执行 child workspace 幂等清理并保留 handoff 审计；通过文件占用、Git 失败、取消竞态和进程重启测试
- [x] 5.5 验证 workspace/handoff 日志、事件、数据库响应和通知不包含凭据、完整命令输出或受控绝对路径；运行 secret scan 与专用脱敏测试并确认通过

## 6. OpenAPI 与 Angular 用户体验

- [x] 6.1 扩展 Rust DTO 和 `utoipa` schema，覆盖 continuation 权限能力、创建/查询/取消、task/run 谱系、策略和安全错误，重新生成 `docs/openapi.json`；验证 OpenAPI snapshot 与后端路由契约测试无漂移
- [x] 6.2 重新生成 Angular API client，并按 Angular Model/Service 模板接入 task/run 详情的数据加载、SSE 刷新和错误映射；验证生成文件无手工漂移且 TypeScript typecheck 通过
- [x] 6.3 在任务详情增加 continuation 时间线、状态、序号、来源/child run 跳转及“追加上下文”操作，按后端能力和权限显隐；通过 Vitest 覆盖加载、空态、策略禁用、提交、重复请求和中文错误
- [x] 6.4 在运行详情展示 primary/retry/continuation/follow-up 区分、父 run/turn 和 handoff 可用性，并仅对未启动请求显示取消；通过 Vitest 覆盖终态不变、SSE 更新、取消冲突和跨范围安全结果
- [x] 6.5 完成追加上下文表单的长度计数、禁用/提交状态、焦点恢复、ARIA 标签和移动端布局，所有可见文案使用简体中文；通过 Angular 可访问性测试及 Playwright 桌面/移动视口验收

## 7. 可观测性与全面验收

- [x] 7.1 增加 `arc_admin_*` continuation 请求量、pending 深度、派发延迟、claim 冲突、重放、拒绝、取消、恢复和 child 结果指标，并关联 request/source run/child run/workspace trace；验证标签低基数且日志不含完整上下文或路径
- [x] 7.2 执行真实 PostgreSQL 集成套件，覆盖组织隔离、幂等创建、多 worker claim、任务状态投影、取消竞态、重启恢复、唯一 child run 和 handoff 不可变；验证隔离 `TEST_DATABASE_URL` 下全部通过
- [x] 7.3 执行受控假 app-server 与 workspace 集成套件，覆盖同 thread 新 turn、断流不误建 continuation、retry/follow-up 分离、来源 cleanup 后重建和重复终态；验证 Agent 不重复启动且敏感数据不落盘
- [x] 7.4 执行 `cargo flow verify --components backend`，修复 secret scan、审计、Clippy、编译和 Rust 测试问题，验证 backend 组件输出通过
- [x] 7.5 执行 `cargo flow verify --components frontend`，修复 lint、typecheck、Vitest、build、可访问性和 Playwright 问题，验证 frontend 组件输出通过
- [x] 7.6 执行 `cargo flow verify --all`、`openspec validate --all --strict` 和 continuation 运行演练，验证完整门禁输出 `TEST_SUMMARY: PASS` 且所有 spec scenario 有可追踪测试证据

## 8. 文档与远端交付

- [x] 8.1 更新总需求、Symphony 证据矩阵、架构、实现状态和 HANDOFF，将 continuation 从计划中改为与实际测试证据一致的状态；验证需求 ID、代码/迁移/测试链接和未完成清单可复查
- [x] 8.2 更新 Orchestrator 运维手册，说明策略默认关闭、handoff 指标、claim/reconciliation、告警、分阶段启用、历史 run 限制和回滚；验证配置示例与实现默认值一致且文档链接检查通过
- [x] 8.3 使用明确文件清单提交并推送；已验证 `feat/continuation-turns` 的 PR #84 于 2026-08-28 合并到 `main`，合并提交为 `acb3bc05c642a5fe7dedcb930d105f5f6a4587bf`，PR URL 为 `https://github.com/musutrade/DevRail/pull/84`
- [x] 8.4 已复查 PR #84 的交付记录：`CI`、`Supply chain security` 和 `arc-flow platform` workflow 均为 `success`；`CodeQL` workflow 为平台主动 `skipped`，无失败 run。`main` 当前未配置 required status checks（GitHub Branch Protection API 返回未保护），因此不存在未通过的 required check；生产分支保护仍按 HANDOFF 的生产事项执行
