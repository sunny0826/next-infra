# 串行 Goal 与并行任务

本文定义已经获准的只读首版开发顺序。用户于 2026-08-02 授权初始化 Git 并按 [`docs/tasks/`](../tasks/README.md) 开始实现；授权不包含真实凭据、Agent 用户配置、发布、公证或外部基础设施写操作。Goal 验收门必须串行完成；前一 Goal 未达到验收标准时，不进入后一 Goal。一个 Goal 内部只有在共享契约冻结后，才可按依赖波次与独占文件所有权并行派发给 `luna_worker`。

## 通用完成标准

每个目标完成时必须报告：

- 实际修改文件。
- 与目标直接对应的行为变化。
- 执行的验证命令及结果。
- 未执行验证及原因。
- 基线问题与本目标引入问题的区分。
- 下一目标是否已具备前置条件。

任何目标都必须遵守：

- 不借机重构无关文件。
- 不存放真实凭据、主机名、IP、仓库私有数据或 Provider 响应 Fixture。
- 所有 shell 命令按仓库规则使用 `rtk` 前缀。
- 外部写操作不属于 Goal 0-9。
- Tauri Desktop Adapter 保持薄层；Core、Store、Sync、Query 禁止依赖 Tauri。
- 同一文件同时只能有一个 owner；共享 manifest、lockfile、migration、生成 DTO 和 entrypoint 由指定 Contract Owner 或 Gate Captain 串行修改。
- worker 需要越过任务包独占路径或改变共享契约时必须停止并回报，不能机会式扩展 scope。

## Goal 0：设计基线冻结

**状态：** Completed  
**依赖：** 无

### 范围

- 确认单机、单用户、本机运行和 React/TypeScript 前端。
- 确认 Tauri v2 Desktop Host + Rust Control Plane Runtime + MCP Bridge 架构。
- 记录本地环境与阻塞项。
- 固化主 RFC、资源模型、Connector 契约、MCP 与安全设计。
- 固化结构图与规范术语表，明确组件所有权、调用方向和歧义词。
- 固化界面信息架构、视觉状态语义和有界 Topology 规则。
- 完成一次独立文档 Review 并记录残留风险。
- 明确后续串行 Goal、Goal 内并行波次、任务依赖、独占路径和 Gate Captain。

### 验收标准

- 本阶段只产生设计、任务拆解、Interface System 和独立 HTML 原型；不产生工程实现。
- 不存在 Cargo、Node、数据库迁移或源码文件。
- 所有文档均不把 PostgreSQL、Docker、多用户、浏览器 UI、loopback HTTP 或远程 MCP 作为首版依赖。
- Tauri、Runtime、Desktop Adapter、Query Service 和 MCP Bridge 术语一致。
- 文档交叉引用有效，Review 中无未处理的阻塞级发现。
- 用户评审并明确授权后，才允许进入 Goal 1。
- 所有工程任务在授权前均保持 `HELD-AUTH`，不会因任务已经拆解而自动变为可执行。

### 验证命令

```bash
rtk proxy find docs/design -maxdepth 2 -type f -print
rtk proxy rg -n "PostgreSQL|Docker|浏览器 UI|loopback HTTP|remote MCP|多用户" docs/design
rtk proxy rg -n "daemon|HTTP API|SSE|CSRF|CORS" docs/design
rtk proxy find . -maxdepth 3 -type f -print
```

## Goal 1：Tauri 工程骨架与发布边界

**状态：** Completed / `GATE-G1 PASS`（2026-08-03）  
**依赖：** Goal 0 获得用户批准

### 开始前必须冻结

以下事项均已冻结，状态索引见 [`docs/design/decisions/`](./decisions/README.md)：

- Tauri 与官方插件版本。
- `next-infra-mcp` 的稳定安装路径。
- Desktop Host 与 MCP Bridge 的协议版本和原子升级策略。
- MCP Bridge 自动拉起 Desktop Host 的授权、可信路径和显式退出抑制规则。
- 自动登录启动的默认值。
- 首版签名、公证和更新边界。
- Keychain item 命名、签名访问控制和后台可用性策略。

### 范围

- 初始化 Git。
- 创建 Rust workspace、Tauri v2 Desktop App 和 React/TypeScript 前端。
- 创建空的 MCP Bridge 可执行目标，但不实现 MCP 工具。
- 固定 Rust、Node/pnpm 和 Tauri 版本。
- 建立最小 lint、test、format、desktop build 命令。
- 建立 crate 依赖守卫，确保 Core 不依赖 Tauri。
- 不创建真实 Connector 和数据库表。

### 验收标准

- Tauri App 能构建、启动、显示主窗口和退出。
- React SPA 只能通过 Mock/Empty Desktop Adapter 启动。
- Rust Core crate 可以在不启动 Tauri 的情况下测试。
- Rust Query DTO 可以生成或校验 TypeScript binding，不依赖手工重复 schema。
- MCP Bridge 是独立二进制目标，且安装位置设计已被验证。
- 所有版本由配置文件固定，不依赖全局漂移。
- 没有 Provider SDK 和外部凭据。

### 验证命令候选

```bash
rtk cargo fmt --all --check
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo test --workspace --all-targets --locked
rtk pnpm --dir apps/desktop lint
rtk pnpm --dir apps/desktop test
rtk pnpm --dir apps/desktop build
rtk pnpm --dir apps/desktop tauri build
```

## Goal 2：领域模型与 SQLite 投影

**状态：** Completed / `GATE-G2 PASS`（2026-08-03）  
**依赖：** Goal 1

### 范围

- 实现 Connection、Resource、ResourceVersion、Relation、SyncRun、Change 和 Coverage。
- 实现 SQLite migration、WAL、自检、单 Writer 和只读 Query。
- 实现 Fixture Connector，不连接真实 Provider。
- 实现变化时才保存版本、partial 不 tombstone 的规则。
- 实现 interrupted SyncRun 和崩溃恢复语义。

### 验收标准

- 重复相同批次不增加版本。
- 全量、增量、partial、targeted 的删除语义通过测试。
- 事务失败不前移 cursor。
- 并行读取结果通过单 Writer 一致提交。
- 临时 SQLite 集成测试不依赖系统 FTS5。
- Core/Store 测试不启动 Tauri 或 WebView。

### 验证命令候选

```bash
rtk cargo test -p next-infra-core
rtk cargo test -p next-infra-store
rtk cargo test -p next-infra-sync
rtk cargo clippy --workspace --all-targets -- -D warnings
```

## Goal 3：Tauri 生命周期、Query Adapter 与 UI 纵切

**状态：** Authorized / In progress  
**依赖：** Goal 2

### 范围

- 实现统一 Query Service。
- 实现薄 Tauri Commands 和 invalidation-only Events。
- 实现单实例、托盘、关闭窗口不退出和显式优雅退出。
- 实现自动登录启动开关和后台启动模式。
- 实现睡眠/唤醒 catch-up 语义。
- 实现 Overview、Inventory、Resource Detail 和有界 Topology 的最小可视化。
- 数据仅来自 Fixture Connector。

### 验收标准

- UI 能展示 Resource Health、Freshness、Connection 和观察时间。
- Fixture Topology 能展示 Provider、configured、inferred 三种证据，不出现无边界全局关系图。
- Event 只携带版本/scope；UI 通过 Command 重新查询。
- Command 查询有分页、字段和拓扑硬上限。
- 关闭主窗口后 Runtime、Writer 和 Fixture 调度继续工作。
- 重开窗口后 UI 从 Query Service 恢复，不依赖旧 React 状态。
- 第二次启动只激活已有实例，不创建第二个 SQLite owner。
- 显式退出会停止同步、排空 Writer 并 checkpoint。
- React 组件测试使用 Mock Desktop Adapter；真实 Tauri macOS smoke 覆盖托盘、关闭与恢复。

### 验证命令候选

```bash
rtk cargo test -p next-infra-query
rtk cargo test -p next-infra-desktop-adapter
rtk pnpm --dir apps/desktop test
rtk pnpm --dir apps/desktop build
rtk test pnpm --dir apps/desktop test:desktop-smoke
```

## Goal 4：Unix Socket 与 STDIO MCP 纵切

**依赖：** Goal 3

### 范围

- 实现版本化 Unix Socket RPC 和 owner/遗留 Socket 校验。
- 实现独立 `next-infra-mcp` STDIO Bridge。
- 实现七个只读 MCP 工具及响应边界。
- 实现 Host/Bridge 协议握手与版本不兼容错误。
- 先在 Codex 上验收；Hermes 未安装时明确记录阻塞。

### 验收标准

- MCP Bridge 不直接读取 SQLite、Keychain 或 Connector。
- MCP 结果与 Desktop Query Service 语义一致。
- 所有工具标记 read-only。
- Topology、列表和 Change 返回有硬上限及截断信息。
- 未授权自动拉起时返回 `host_unavailable`；已授权时只能启动安装记录中的可信 App 并等待同一个 Runtime。
- 用户显式退出 Host 后，当前和新建 Bridge 进程都不能清除 `user_quit` 标记或循环重新拉起应用。
- Bridge/Host 不兼容时拒绝连接。
- Codex 完成真实查询验收。

### 验证命令候选

```bash
rtk cargo test -p next-infra-local-rpc
rtk cargo test -p next-infra-mcp
rtk proxy codex mcp add --help
```

Hermes 的实际命令必须在安装后根据当时版本重新确认，不提前写死为已通过。

## Goal 5：GitHub / Actions Connector

**依赖：** Goal 4

### 范围

- 实现细粒度只读认证和 Keychain SecretRef。
- 读取 Repository、Workflow、Run、Job、Environment、Deployment 摘要。
- 实现分页、ETag、rate-limit 和字段清洗。
- 不保存完整日志、工件或 Secret。

### 验收标准

- Fixture Contract Test 全部通过。
- 私有信息不进入测试 Fixture、日志和 SQLite。
- Desktop UI 与 MCP 能查看 Repo → Workflow → Run 关系。
- 429、权限不足和部分分页不会触发误删除。

## Goal 6：SSH / Mac mini Connector

**依赖：** Goal 5

### 范围

- 复用系统 OpenSSH 和用户 SSH config alias。
- 实现固定、版本化、有限输出的 host 探针。
- 支持 macOS launchd 和 Linux systemd 服务摘要。
- 不实现任意命令工具。

### 验收标准

- Host Key 不匹配立即失败且不自动接受。
- 每个探针有超时、输出上限和独立错误。
- MCP 和 Tauri Commands 均无法提交命令文本。
- 远程不可达只影响 Freshness/Connector Health，不把主机伪装成 down。

## Goal 7：Topology、Binding 与 Timeline

**依赖：** Goal 6

### 范围

- 实现人工 Binding。
- 实现 Provider、configured、inferred 三类关系证据。
- 实现有界 Topology UI 和结构化 Change Timeline。
- 不做自动资源合并。

### 验收标准

- 图默认 depth 1，硬上限生效。
- 推断关系显示证据与 confidence。
- Binding 端点失效时标记 unresolved，不静默删除。
- Timeline 不显示重复未变化轮询。

## Goal 8：Dokploy 与 Cloudflare Connector

**依赖：** Goal 7

### 范围

- 实现 Dokploy Project/Application/Deployment/Server/Domain。
- 实现 Cloudflare Account/Zone/DNS/Tunnel/Worker 摘要。
- 建立 Repo → Deployment → Host → DNS 的端到端拓扑。

### 验收标准

- Dokploy 数据库密码等敏感字段在 Connector DTO 层即被排除。
- Cloudflare 使用资源限定的只读 Token。
- 跨平台关系的来源在 UI/MCP 可见。
- 端到端拓扑能在有界查询中重放。

## Goal 9：Supabase 与云厂商基础覆盖

**依赖：** Goal 8

### 范围

- 分别实现 Supabase managed 和 self-hosted Connector。
- 实现阿里云/腾讯云计算、网络、负载均衡和 DNS 的首批模块。
- 实现 Connector Coverage Matrix UI。

### 验收标准

- 两类 Supabase 不共享错误的控制面假设。
- 每个云产品模块单独显示支持状态。
- 不使用“支持整个云厂商”作为验收结论。
- Provider 限流、权限范围和区域分页经过真实只读账户验证。

## Goal 10：操作能力 RFC

**依赖：** Goal 0-9 的只读系统稳定；需要新的用户授权

### 范围

只创建设计，不实现操作：

- Action Connector。
- Plan、Diff、Approval、Execute、Verify。
- 独立 Admin MCP。
- 风险分级、幂等、审计和回读验证。

### 验收标准

- 操作 RFC 通过独立评审。
- 只读凭据不能自动升级为写凭据。
- 审批不是布尔值。
- 任意 SSH 命令和删除操作不进入第一批 Action。

## 当前推进与停止条件

- Goal 1 已获授权，可以按任务表实现；每个后续 Goal 只有在前一 Gate 通过后才可开始。
- 本次授权覆盖 Goal 0-9 的本地、只读实现，但不因通过 Gate 自动扩大到真实凭据录入、Codex/Hermes 用户配置修改、安装、发布或公证；这些外部状态变更仍需独立授权。
- Goal 10 仍只允许设计，任何外部写操作都必须等待新的明确授权。
- 若 Gate 发现 blocker，应修复并重新验收，不能为了保持进度跳过或降级标准。
