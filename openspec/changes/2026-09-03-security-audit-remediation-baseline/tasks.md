## 1. 设计定案与追踪基线

- [ ] 1.1 在实现前确定 repair lease 的 grace window、续租间隔、取消信号和指标命名，并将决定记录到本 change 的 design.md；验证设计评审记录与 `controlled-repair-runs` 规格一致
- [ ] 1.2 在实现前确定 TOTP challenge 防重放采用 challenge 字段扩展还是独立按用途表，定义默认值、清理和回滚策略；验证数据库设计记录不改变重新认证计数器语义
- [ ] 1.3 建立审计条目追踪表，将 P1/P2 条目绑定到本 change 或后续拆分的 OpenSpec change、代码范围、迁移、测试、门禁和关闭证据；验证每个条目都有唯一追踪 ID

## 2. Repair 门禁租约与并发安全

- [ ] 2.1 为 repair gate rerun 增加按 owner/token 续租的执行生命周期，并确保续租请求只更新仍由当前 owner 持有的 claim；验证真实 PostgreSQL 测试覆盖有效续租和错误 token 拒绝
- [ ] 2.2 将门��命令执行、租约 keepalive、取消和进程终止组成可观察的并发流程；验证 worker 失去租约后不会继续接受结果写入
- [ ] 2.3 加固完成、失败、取消、释放、过期回收和接管路径的条件更新与 `rows_affected` 处理；验证旧 worker 的迟到结果不会覆盖新 owner 的结果或任务投影
- [ ] 2.4 增加两个 worker 竞争同一 rerun、同一 cwd 和重复终态事件的 PostgreSQL 回归测试；验证任一时刻最多一个有效执行者且不会重复产生产物/通知
- [ ] 2.5 验证 repair lease 迁移在空库和已有数据上正向执行，并检查迁移锁、索引和回滚前置条件；交付可复核迁移输出

## 3. TOTP 与授权数据范围

- [ ] 3.1 实现登录、TOTP 注册和重新认证的用途分域消费状态，并保持同一 challenge 并发提交最多一个成功；验证 Rust/真实 PostgreSQL 测试覆盖成功、重放、过期和并发竞争
- [ ] 3.2 收紧 `add_member` 的项目、用户和组织/部门/所有者范围谓词及安全响应；验证跨组织枚举和范围外添加均不写入且返回不可枚举结果
- [ ] 3.3 在任务和评审创建/更新时校验 `assignee_user_id` 与 `reviewer_user_id`；验证同范围成功、跨部门拒绝和跨组织拒绝
- [ ] 3.4 修正可信代理下畸形、超长和不可解析 `X-Forwarded-For` 的客户端归因；验证可信链、畸形链、不可信 peer 和限流隔离测试
- [ ] 3.5 将 Webhook 与 repair 回调密钥加入示例配置和生产 compose/secrets 契约，保持空值失败关闭；验证配置检查不会接受空白值或默认 HMAC

## 4. 供应链与 CI 门禁

- [ ] 4.1 将所有第三方 Actions 固定为完整 commit SHA，并同步更新供应链检查脚本的允许映射；验证可变 tag、未知 SHA 和错误 action 身份均被门禁拒绝
- [ ] 4.2 为生产基础镜像和运行时镜像固定 digest，并让构建证据记录实际镜像身份；验证重复构建和扫描输入使用同一 digest
- [ ] 4.3 扩展安全变更过滤范围以覆盖 `deployment/nginx/**`，移除无匹配的 `docker/**` 规则；验证 nginx 变更触发 Trivy/SBOM
- [ ] 4.4 拆分或调整 schedule 逻辑，使默认分支无源代码变更时仍执行依赖和镜像安全扫描；验证 schedule 模拟运行不会因路径过滤跳过
- [ ] 4.5 增加 RUSTSEC 风险接受的 UTC 到期检查，覆盖 2026-12-31 00:00:00 前后行为；验证到期后仍存在 ignore 时本地和 CI 检查均失败
- [ ] 4.6 验证 CodeQL、dependency review、RustSec、Trivy 和 SBOM job 的实际执行条件与 required checks；交付一次包含执行 job、跳过原因和 artifact 的 CI 运行

## 5. 前端安全边界

- [ ] 5.1 为 run/task SSE 统一保存连接、重连定时器和销毁状态，并实现异常后的固定重连或明确不可用状态；验证 Chromium 桌面和移动测试覆盖销毁窗口、重复错误和代理断开
- [ ] 5.2 将主题初始化移入 Angular 启动代码并移除生产 CSP 会拦截的内联依赖；验证生产 CSP 响应无违规且运行时应用名称按配置生效
- [ ] 5.3 增加共享 URL、文件名和 deep link 白名单校验，阻止危险协议、未知路由、非法字符和超长值进入 DOM/Router；验证安全值成功、危险值拒绝和用户可见错误
- [ ] 5.4 将 5xx 错误转换为通用简体中文文案并保留受约束 trace ID；验证服务端内部消息、路径和数据库细节不出现在 snackbar、横幅或页面正文

## 6. 审计证据与文档一致性

- [ ] 6.1 设计可提交的安全证据摘要格式，至少包含 UTC 时间、提交 SHA、scope、完整命令、结果摘要、失败详情和 artifact 定位；验证从干净 clone 可读取格式说明
- [ ] 6.2 将门禁/审计摘要归档到版本控制目录或稳定 CI artifact，移除对 gitignore 的 `test_result.md` 的唯一依赖；验证断开本机 reports 目录后文档仍可复核
- [ ] 6.3 为每个实现 OpenSpec change 更新审计条目、ADR、PR、测试和关闭证据关联；验证追踪矩阵不存在无 owner、无证据或重复关闭状态
- [ ] 6.4 检查状态文档、HANDOFF、MVP 验收矩阵、ADR 和审计复核中的“已加入/已通过”声明；验证冲突声明会触发文档一致性门禁

## 7. 集成验证与交付

- [ ] 7.1 为后端变更运行 `cargo flow scope` 并按范围执行 Rust reviewer、格式、Clippy、编译和真实 PostgreSQL 测试；交付脱敏命令输出和测试摘要
- [ ] 7.2 为前端变更运行 `cargo flow scope` 并执行 Angular 单元测试、桌面/移动 Chromium 回归和生产构建；交付脱敏命令输出与浏览器 artifact
- [ ] 7.3 执行 `openspec validate --all --strict`，确认本 change 的 proposal、五个新 capability spec、controlled-repair-runs delta、design 和 tasks 全部有效
- [ ] 7.4 执行 `cargo flow verify --all`，确认 secret scan、架构审计、后端/前端门禁、供应链和 arc-flow 检查全部通过；保留带提交 SHA 的 CI artifact
- [ ] 7.5 以新的 `main` 为基线重新执行安全审计复核，逐项更新本 ADR-0010 追踪矩阵；验证所有条目达到“已验证”或有明确 owner、期限和风险接受
- [ ] 7.6 由维护者审阅迁移、回滚、生产/浏览器证据和文档一致性后，才可将 ADR-0010 状态从 Proposed 更新为 Accepted；验证变更前 ADR 仍保持 Proposed
