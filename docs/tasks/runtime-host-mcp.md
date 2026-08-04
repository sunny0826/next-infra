# Runtime、Host 与 MCP 任务包

本文件覆盖 Rust Domain/Store/Sync/Query/Runtime、Tauri Desktop Host、Keychain、Local RPC 和 STDIO MCP。通用状态、角色、派发格式及 Gate 规则见[总调度手册](./README.md)。`RHM-G2-01/02` 与 `CON-G2-01` 已进入复核，当前可并行派发 Projection Store、Sync Engine、Normalizer、Fixture 与 Contract/Catalog 分支。

## Goal 1：工程与发布骨架

### `RHM-G1-01` — Workspace Bootstrap 与空发布目标

- **状态：** `REVIEW`。
- **目标：** 建立可编译、无业务实现的 Rust workspace、Tauri App、React host 和独立 MCP Bridge target。
- **依赖：** 用户授权进入 Goal 1；`DEC-G1-06` 已给出无 blocker 的 `READY` 结论。
- **独占路径：** 根 manifests/lockfiles/toolchain/ignore 配置；所有初始 crate/app manifests；全部空 crate `src/lib.rs`；`apps/mcp-bridge/src/main.rs`；Goal 1 的 `apps/desktop/src-tauri/{build.rs,src/main.rs,tauri.conf.json,capabilities/**,icons/icon.png}`；依赖与 bundle guard 的 `apps/desktop/scripts/**`；以及可由后续 Shell Owner 接管的最小 `apps/desktop/{index.html,vite.config.ts,tsconfig*.json,src/main.tsx,src/main.test.tsx,src/test/setup.ts,src/vite-env.d.ts}`。本任务同时是 Goal 1 Bootstrap Captain 与临时 Desktop Composition Captain。
- **只读输入：** crate 拓扑、版本矩阵、Bridge 安装和签名决策。
- **范围：** 空 crates/targets、工具链固定、依赖方向 guard、最小 React/Vite host、最小 Tauri composition、lint/test/build 和 bundle-boundary 命令。
- **非目标：** 不创建数据库表、真实 Connector、MCP Tool、业务页面或 Secret。
- **输出：** 可构建 workspace、独立 Core 测试目标、独立 `next-infra-mcp` 二进制目标、可显示空主窗口的 React/Tauri host，以及逐路径 handoff 清单。完成后，React Shell 路径交给 `UI-G1-02`；Tauri composition 路径在 Goal 1 Gate 后保持冻结，Goal 3 再交给 `RHM-G3-05`。
- **验收：** Core 不依赖 Tauri；Bridge 不成为 Desktop 默认 binary；无 Provider SDK；版本不依赖全局漂移。
- **验证：** `rtk cargo metadata --locked --format-version 1`；`rtk cargo test -p next-infra-core --locked`；`rtk cargo test --workspace --all-targets --locked`；`rtk cargo clippy --workspace --all-targets --locked -- -D warnings`；Desktop frozen install/lint/test/build；独立 Bridge build；Tauri build；`test:bundle-boundary`。QDTO export/drift 在 `UI-G1-01` 完成后由 `GATE-G1` 执行。
- **实现证据（2026-08-02）：** workspace、独立 Bridge、最小 React/Tauri host、依赖闭包 guard、受限 CSP 与 bundle-boundary guard 均已落地；上述验证全部通过，最终 `.app` 已完成真实启动、进程确认与正常退出回归。
- **风险/停止：** 根 manifest 和 lockfile 禁止其他 worker 同时编辑；发现 crate 图与决策不一致时停止，不自行改设计。

## Goal 2：领域与 SQLite

### `RHM-G2-01` — Domain Contract 冻结

- **状态：** `REVIEW`。
- **目标：** 冻结所有下游共享的领域类型和 ports。
- **依赖：** `GATE-G1` 通过。
- **独占路径：** `crates/next-infra-core/**`。
- **范围：** Connection、Resource、ResourceVersion、Relation、RelationVersion、SyncRun、Change、Coverage、Health、Freshness、Lifecycle、Fingerprint、结构化错误，以及 Store/Connector/Secret ports。
- **非目标：** 无 SQL、Provider SDK、Tauri、MCP、Binding UI 或推断实现。
- **输入/输出：** 资源模型和术语表 → 版本化 Rust types、invariant tests、契约变更流程。
- **验收：** Provider/Connector/Connection 不混淆；Health/Freshness/两种 Coverage 分离；SecretRef 与 Secret 值无法混用；Provider kind 不成为无限增长大枚举。
- **验证：** `rtk cargo test -p next-infra-core`；`rtk cargo clippy -p next-infra-core --all-targets -- -D warnings`。
- **实现证据（2026-08-03）：** 已实现强类型 ID/kind、Connection/Resource/Version/Relation/Binding/SyncRun/Change、独立 Resource/Connector Health、Freshness、Lifecycle、两类 Coverage、结构化 Evidence/Origin/Error，以及 Store/Connector/Secret ports；SecretValue 不可序列化且 Debug 脱敏。6 项黑盒不变量测试、Core Clippy `-D warnings` 和全 workspace 11 项测试通过。
- **风险/停止：** 下游不得直接修改 Core；缺字段时提交契约请求并等待 owner 串行处理。

### `RHM-G2-02` — SQLite 基础与 Migration

- **状态：** `REVIEW`。
- **目标：** 建立安全、可恢复且不依赖系统 FTS5 的 SQLite schema 基础。
- **依赖：** `RHM-G2-01`。
- **独占路径：** `crates/next-infra-store/src/migrations/**`、Store 初始化和 migration tests。
- **范围：** migration、WAL、foreign keys、busy timeout、数据目录权限、自检、checkpoint/backup 基础。
- **非目标：** 不实现 Sync 判断，不保存 Provider 原始响应。
- **输入/输出：** Store ports 和逻辑模型 → 可在临时目录重复启动的 schema。
- **验收：** migration 幂等启动；损坏或 migration 失败时拒绝写入；不依赖 FTS5。
- **验证：** `rtk cargo test -p next-infra-store migrations`。
- **实现证据（2026-08-03）：** 已固定 `rusqlite 0.39.0` bundled SQLite（0.40.1 因使用 Rust 1.92 尚未稳定的 `cfg_select!` 被拒绝），实现用户独占目录/数据库权限、WAL、foreign keys、5 秒 busy timeout、quick check、STRICT schema、事务化 user_version migration 和未来 schema 拒绝。6 项测试覆盖幂等重启、损坏数据库、无 FTS 依赖和 Unix 0700/0600 权限，专属 Clippy 通过。
- **风险/停止：** migration 编号单写；已合并 migration 不由其他任务改写。

### `RHM-G2-03` — SQLite Projection Store

- **状态：** `REVIEW`。
- **目标：** 实现当前投影、有限历史和原子批次提交。
- **依赖：** `RHM-G2-01/02`。
- **独占路径：** `crates/next-infra-store/**`，排除 migration owner 的冻结文件。
- **范围：** Resource/Relation/Version/Change/SyncRun/cursor 读写、只读连接和维护入口。
- **非目标：** 不调度 Connector，不把 SQL 暴露给 Tauri/MCP，不自行决定 tombstone 业务规则。
- **输入/输出：** Store ports/schema → SQLite adapter 与集成测试。
- **验收：** 事务失败不前移 cursor；读者看不到半批次；未变化事实不新增版本。
- **验证：** `rtk cargo test -p next-infra-store`。
- **实现证据（2026-08-03）：** Store 已实现 Connection/SyncRun/Resource/Relation/Version/Change/cursor 与按 scope missing evidence 的读写和单事务 `SyncCommit`，提交前校验 cursor 与 provenance，稳定身份不可静默变化；外键失败测试证明投影、版本、cursor 和 missing state 全部回滚。Store 还能确定性读取最新 RelationVersion fingerprint，支持关系版本去重。12 项 Store 测试、专属 Clippy 和全 workspace 回归通过。
- **风险/停止：** Sync Coverage 和 tombstone 规则不能下沉为隐蔽 SQL 特例。

### `RHM-G2-04` — Writer 与 Sync Engine

- **状态：** `REVIEW`。
- **目标：** 将已验证 ObservationBatch 通过唯一 Writer 原子提交。
- **依赖：** `RHM-G2-01`、`CON-G2-01` Connector API；使用 fake Store 时可与 `RHM-G2-02/03` 并行；真实集成还依赖 `CON-G2-02` Normalizer。
- **独占路径：** `crates/next-infra-sync/**`。
- **范围：** Writer queue、SyncRun 生命周期、cursor、diff、Coverage、两次权威缺失 tombstone、启动恢复和调度接口。
- **非目标：** 无真实 Provider、无 SQLite 直连、无外部写操作、Normalizer 不在本 crate 重复实现。
- **输入/输出：** `ValidatedBatch`、Store port → 可由 Fixture 重放的 Sync Engine。
- **验收：** 相同批次不新增 Version；partial/incremental/targeted/failed 不增加缺失计数；仅同 scope 连续两次成功 authoritative full 可 tombstone；遗留 running 恢复为 interrupted。
- **验证：** `rtk cargo test -p next-infra-sync`。
- **实现证据（2026-08-03）：** 已实现 FIFO 单 Writer、`SyncRunStart`/生命周期、Coverage 校验、cursor、资源与关系 diff/version、按 scope missing evidence、两次权威缺失 tombstone、重现恢复 active 和启动 interrupted 恢复。相同 Resource/Relation fingerprint 不新增版本；Writer 事务失败时保留队首任务供重试；partial/incremental/targeted/failed 不增加缺失计数。10 项 Sync 测试、严格 Clippy 与 workspace 回归通过。
- **风险/停止：** Connector 不得取得 Store 写连接；需要改变 `ValidatedBatch` 时回派 Normalizer owner。

### `RHM-G2-05` — 原子性与恢复集成证据

- **状态：** `REVIEW`。
- **目标：** 用真实临时 SQLite 串联 Fixture、Normalizer、Writer 与 Store，向 `GATE-G2` 提供自动化证据。
- **依赖：** `RHM-G2-03/04`、`CON-G2-02/03/04`。
- **独占路径：** `tests/integration/store_sync/**`；共享 manifest 由 `GATE-G2` Captain 修改。
- **范围：** 并行观察、单 Writer、回滚、cursor、版本、tombstone、partial 和崩溃恢复。
- **非目标：** 验收 worker 不修改生产模块。
- **输出：** 可重复的 Goal 2 gate suite 和失败归属表。
- **验收：** [`implementation-goals.md`](../design/implementation-goals.md) 的 Goal 2 行为均有证据。
- **验证：** Core、Store、Sync 和 connector pipeline tests。
- **实现证据（2026-08-03）：** `next-infra-store-sync-integration` 使用真实临时 SQLite 覆盖外键失败原子回滚、相同批次不新增 ResourceVersion、连续两次 authoritative missing 后 tombstone、重现恢复 active、partial/incremental/targeted/failed 的 missing/cursor 语义，以及 running run 恢复为 interrupted。5 项集成测试和专属 Clippy 通过。
- **风险/停止：** 失败回派对应 owner，不在集成测试中绕过契约。

## Goal 3：Query、Runtime 与 Desktop Host

### `RHM-G3-01` — Query Service 与 QDTO v1 冻结

- **状态：** `REVIEW`。
- **目标：** 建立 Desktop 与 MCP 共用的唯一查询语义。
- **依赖：** `GATE-G2` 通过；Goal 1 的最小 binding pipeline 可用。
- **独占路径：** `crates/next-infra-query/**`、权威 QDTO schema/生成器、`apps/desktop/src/generated/query/**`。
- **范围：** resource search/detail、connection snapshot、topology、health、changes、sync、coverage、分页、字段过滤、snapshot version、Frontier、错误清洗和 TS binding drift。
- **非目标：** 无 Tauri Command、MCP Content、UI 或 Provider 特例。
- **输入/输出：** Store read port → 版本化 Query DTO、Service、generated bindings 和 tests。
- **验收：** Health/Freshness/Connector Health 分离；Topology 默认与硬上限生效；cursor opaque 且稳定；Secret 不进入 DTO。
- **验证：** `rtk cargo test -p next-infra-query`、binding drift check、Desktop TypeScript build。
- **实现证据（2026-08-03）：** Query crate 已冻结 Desktop/MCP 共用的八个入口：resource search/detail、bounded connection snapshot、bounded topology、health summary、recent changes、sync status 与 connector coverage。`QuerySource` 每个 method 原子返回 metadata/body；Service 统一 snapshot metadata、include/filter plan、默认/硬上限、`niq1:` opaque cursor、not-found/source/contract error 清洗。Connection snapshot 硬上限 200；Topology 默认 depth 1、100 nodes/200 edges，硬上限 depth 3、200/400，并保留 frontier/truncated。QDTO 确定性生成 40 个 TypeScript bindings；Relation evidence、Connection snapshot、三类 health 计数、Detail/Page/Topology DTO 均不含 Secret。13 项 Query tests、严格 Clippy、binding export、Desktop lint/test/build 和 workspace 回归通过。
- **风险/停止：** Desktop/MCP 可有不同 transport projection，但不得重定义 Query 语义。

### `RHM-G3-02` — Control Plane Runtime

- **状态：** `REVIEW`。
- **目标：** 将 Store、Writer、Query、Scheduler 和维护组合为不依赖 Tauri 的可测试 Runtime。
- **依赖：** `RHM-G3-01` 的 Query handle 和冻结后的 Runtime API；可与 Host/Keychain/UI 基础并行。
- **独占路径：** `crates/next-infra-runtime/**`。
- **范围：** interactive/background 启动、恢复、调度、睡眠 catch-up、优雅停止、Writer drain 和 checkpoint。
- **非目标：** 不处理窗口、托盘、Tauri plugin、Socket 或 STDIO。
- **输入/输出：** Core ports 与服务 handles → Runtime start/stop API 和独立集成测试。
- **验收：** 不启动 WebView 可测试；关闭顺序可证明；唤醒后错峰 catch-up，不补跑全部周期。
- **验证：** `rtk cargo test -p next-infra-runtime`。
- **实现证据（2026-08-03）：** Tauri-independent Runtime 已覆盖 interactive/background、startup recovery、admission、确定性调度、sleep/wake 有界 catch-up，以及 drain Writer 后 checkpoint 的停止顺序。`SqliteRuntimeBackend` 现由真实 `SyncEngine<Store>` 持有唯一 WriterQueue/SQLite write owner，startup recovery 调用真实 interrupted recovery，stop 先 flush queue 再执行 WAL truncate checkpoint；临时 SQLite integration 验证 running→interrupted、排空真实 commit、checkpoint 与最终 succeeded 状态。Runtime 9 项测试、Store 13 项测试和严格 Clippy 通过；QueryService 仍作为 Runtime 的独立只读 handle，由 Composition 注入实际 QuerySource。
- **风险/停止：** Runtime 不得变成第二个 daemon，也不得引入 Tauri 类型。

### `RHM-G3-03` — Desktop Host Lifecycle

- **状态：** `REVIEW`。
- **目标：** 实现唯一 Tauri Desktop Host 的 macOS 生命周期。
- **依赖：** `DEC-G1-02`；冻结的 Runtime start/stop trait；可用 fake Runtime 并行。
- **独占路径：** `apps/desktop/src-tauri/src/host/**`；entrypoint/config/capabilities 留给 Composition Captain。
- **范围：** 单实例、close→hide、Dock/托盘恢复、后台启动、autostart 和显式退出状态机。
- **非目标：** 无 MCP auto-launch、Query 业务或 UI 页面。
- **输入/输出：** lifecycle 决策 → Host 模块和状态机 tests。
- **验收：** 第二实例只激活窗口；close 不停止 Runtime；Quit 触发 drain/checkpoint；crash 不写 `user_quit`。
- **验证：** 状态机 unit tests；真实 bundle smoke 由 `RHM-G3-05` 执行。
- **实现证据（2026-08-03）：** 平台无关 HostLifecycle 已覆盖 interactive/login/MCP background launch、close→hide、tray/Dock/second-instance restore、WebView reload re-query、显式 Quit 的 marker→drain→checkpoint→stop，以及 crash 不写 `user_quit`。独立 Review 修复 BackgroundOnly restore 缺少 CreateWindow、可见窗口 restore 缺少 RequeryAfterRestore；Desktop Rust 共 21 项测试与严格 Clippy 通过。真实 Tauri/AppKit effects 仍由 `RHM-G3-05` 串行接入。
- **风险/停止：** WebView reload 不得等同 Runtime restart；close/hide/quit 分支须互斥清晰。

### `RHM-G3-04` — Keychain SecretProvider

- **状态：** `RUNNING / LIVE-SMOKE-BLOCKED-ENVIRONMENT`。
- **目标：** 实现一次性 Secret 写入和运行期临时访问。
- **依赖：** `DEC-G1-04`、Core SecretProvider port。
- **独占路径：** `apps/desktop/src-tauri/src/keychain/**` 和专用 Secret Command tests。
- **范围：** 稳定 item 命名、SecretRef、替换顺序、后台不可用和错误清洗。
- **非目标：** 不返回已有 Secret，不从 CLI 参数或 shell rc 接收 Secret，不实现 Provider 认证。
- **输入/输出：** Connection ID/secret type → SecretRef 与受限 provider access。
- **验收：** Rust 生成 item 名；先写新 item、验证引用、再删旧 item；锁屏返回 `credential_unavailable` 且不循环弹窗；日志/SQLite/DTO 无 Secret。
- **验证：** fake Keychain unit tests + 当前签名身份的 macOS smoke。
- **阶段证据（2026-08-03）：** Core `SecretRef` 已升级为结构化非秘密元数据并可由 SQLite JSON round-trip；SecretManager 已实现 add→no-UI read-back→atomic ref switch→old delete、rollback 与 cleanup_pending。真实 macOS adapter 使用 `SecItemAdd`（不调用 Update）、Data Protection Keychain、显式 access group/service/account、`synchronizable=false`、`WhenUnlockedThisDeviceOnly`，读取使用 `LAContext.interactionNotAllowed=true` 与 `kSecUseAuthenticationContext`，delete 精确限定 namespace；Desktop Rust 21 项测试和严格 Clippy 通过。当前 `security find-identity -p codesigning -v` 为 `0 valid identities found`，因此 Apple Development packaged smoke 尚未执行，ad-hoc 不得替代。
- **风险/停止：** 开发签名、Developer ID 和后台行为不能相互代替验收。

### `RHM-G3-05` — Desktop Composition 与生命周期 Smoke

- **目标：** 串联 Runtime、Adapter、Host 和 Keychain，并验证真实 Tauri bundle。
- **依赖：** `RHM-G3-02..04`、`UI-G3-01` Adapter、Goal 3 UI Shell integration。
- **独占路径：** Desktop shared manifests、`main.rs`、`lib.rs`、`tauri.conf.json`、capabilities 和 composition smoke；本任务是 Goal 3 Desktop Composition Captain。
- **范围：** 注册模块、关闭/恢复、第二实例、显式退出、唯一 DB owner。
- **非目标：** 不修页面视觉，不用 Vite 页面代替 Tauri App。
- **输出：** 可启动真实 bundle 和 lifecycle smoke 结果。
- **验收：** 窗口关闭后 Fixture 调度继续；恢复后 UI 重新 Query；只有一个 Runtime/Writer/SQLite owner。
- **验证：** workspace tests、Desktop tests/build、`tauri build`、`test:desktop-smoke`。
- **风险/停止：** 本任务串行编辑 manifests/entrypoint/capabilities；其他 worker 同时改这些文件时不得开始。

## Goal 4：Local RPC 与 MCP

### `RHM-G4-01` — Local RPC v1 Contract 冻结

- **状态：** `REVIEW`。
- **目标：** 冻结 Host/Bridge 的版本化本地协议。
- **依赖：** `GATE-G3` 通过；Query DTO v1 冻结。
- **独占路径：** `crates/next-infra-local-rpc/src/protocol/**`、protocol golden fixtures。
- **范围：** framing、handshake、request ID、caller、frame/length/concurrency limits、query variants、错误码和兼容策略。
- **非目标：** 不实现 Socket I/O 或 MCP server。
- **输出：** v1 protocol、golden round-trip/compatibility tests。
- **验收：** 明确定义 `host_unavailable`、protocol mismatch、oversized frame；无通用 SQL/Secret/Connector 方法。
- **验证：** Local RPC protocol golden tests。
- **实现证据（2026-08-04）：** 已实现 4-byte BE length-prefixed UTF-8 JSON、1 MiB body/128-byte request ID/8 in-flight 上限、双向 minor/capability 协商、七个 typed read-only Query request/response、八个稳定错误码和 golden/边界测试。主线 Review 修正 `frame_too_large` wire 拼写、相邻 minor 窗口和 serde 反序列化校验；Local RPC 12 项、workspace 133 项测试及严格 Clippy 通过。
- **风险/停止：** framed JSON 与 JSON-RPC 必须先择一，不能让并行 worker各自选择。

### `RHM-G4-02` — Unix Socket RPC

- **状态：** `REVIEW`。
- **目标：** 实现受限 UDS server/client，只适配 Query Service。
- **依赖：** `RHM-G4-01`。
- **独占路径：** `crates/next-infra-local-rpc/**`，排除冻结 protocol/golden 文件。
- **范围：** UDS、framing、peer UID、owner/mode、stale socket 校验、请求限流和 Query adapter。
- **非目标：** 无 TCP/HTTP、SecretProvider 或 Connector endpoint。
- **验收：** parent `0700`、socket `0600`；拒绝 symlink/错误 owner/超限 frame；仅在 owner/process 校验后清理 stale socket。
- **验证：** `rtk cargo test -p next-infra-local-rpc`。
- **阶段证据（2026-08-04）：** Transport foundation 已实现原子 `0700` run dir、`0600` O_NOFOLLOW+flock lock、`0600` socket、active/stale/replacement-safe lifecycle、client/accept path revalidation、macOS `getpeereid` 和 header-first bounded stream framing。Local RPC 20 项测试通过，transport security 7 项连续运行 20 次稳定；handshake session、8 in-flight enforcement 与 Query adapter 仍待 `RHM-G4-02B`。
- **完成证据（2026-08-04）：** Session 已实现 peer UID → ClientHello/typed HandshakeResponse → frozen RequestEnvelope 状态机、七个 QueryService adapter、request ID 回显、8 active worker admission 和串行 response writer；第九个请求不入 queue。真实 owner-only filesystem UDS 与 socket-pair 状态机测试共使 Local RPC 达到 25 项，session suite 多轮重复稳定。
- **风险/停止：** 首版平台验收只承诺冻结的 macOS target；平台差异不能静默放宽安全检查。

### `RHM-G4-03` — 七个只读 STDIO MCP Tool

- **状态：** `REVIEW`。
- **目标：** 实现独立 Bridge 和稳定只读工具面。
- **依赖：** `RHM-G4-01`；可用 fake RPC 与 `RHM-G4-02` 并行。
- **独占路径：** `crates/next-infra-mcp/**`、`apps/mcp-bridge/src/mcp/**`。
- **范围：** `search_resources`、`get_resource`、`get_topology`、`get_health_summary`、`get_recent_changes`、`get_sync_status`、`list_connector_coverage`，以及 MCP Resources/read-only annotations。
- **非目标：** 无 refresh、配置修改、Secret、外部操作或直接 SQLite/Keychain/Connector 访问。
- **验收：** 所有结果有界并含 `observed_at`；Topology 有 `truncated/frontier`；Provider 文本不能改变权限。
- **验证：** `rtk cargo test -p next-infra-mcp`。
- **阶段证据（2026-08-04）：** MCP projection/router 已实现七个强类型工具、structured output、四项 read-only annotations、两个固定 Resources、Local RPC client adapter 与安全错误；MCP 2 项 contract tests 和严格 Clippy 通过。真实 Bridge STDIO child-process composition 与 smoke 仍待 `RHM-G4-03B`。
- **完成证据（2026-08-04）：** 独立 Bridge 已接入 current-thread STDIO、固定 owner-only Local RPC endpoint 和 read-only existing-path；真实 child process 完成 initialize、七工具 list/call、两 Resources list/read、tool error 与纯 JSON stdout smoke，连续执行 20 次稳定。Bridge production dependency surface 未增加 Store/Runtime/Connector/Tauri/HTTP。
- **风险/停止：** 不为 Provider 端点扩张工具数，不在 Bridge 重写 Query 语义。

### `RHM-G4-04` — Host Availability 与 `user_quit`

- **目标：** 实现可信自动拉起与跨 Bridge 抑制状态机。
- **依赖：** `DEC-G1-02/03`、`RHM-G4-01`。
- **独占路径：** Bridge host-availability/trusted-install 模块和 Desktop `user_quit` 模块；entrypoint wiring 留给 Gate Captain。
- **范围：** 预授权、本地安装记录、可信 bundle path/owner/signature、有限等待、持久 `user_quit`。
- **非目标：** Agent 参数不能授权或提供任意 executable path。
- **验收：** 未授权/超时/失败均返回 `host_unavailable`；MCP 不能清除 `user_quit`；新 Bridge 仍受抑制；只有用户启动或已启用的下一次登录启动可清除。
- **验证：** 多进程状态机 tests 和签名 App smoke。
- **风险/停止：** mock 不能代替安装路径、签名和升级验收。

### `RHM-G4-05` — Agent 与安全端到端验收

- **目标：** 验证 Bridge → Socket → Query 的真实终态。
- **依赖：** `RHM-G4-02..04`。
- **独占路径：** Goal 4 E2E/security tests；共享 MCP manifests/entrypoint 由 `GATE-G4` Captain 独占。
- **范围：** protocol mismatch、Host unavailable、auto-launch、socket 权限、七工具边界、Codex 查询；Hermes 可用时再验收。
- **非目标：** 不把未安装 Hermes 写成通过，不默认修改用户级 Codex 配置。
- **验收：** Bridge 无 Store/Keychain/Connector 依赖；无 HTTP listener；Quit 后当前和新 Bridge 均不复活 Host；Codex 真实查询通过。
- **验证：** Local RPC/MCP/workspace tests、`rtk proxy codex mcp add --help`；用户级配置变更需另行授权并提供恢复步骤。
- **风险/停止：** Hermes 未安装标记 `BLOCKED`；Fixture 不能替代真实 Codex 路径。

## Goal 7：Binding、Topology 与 Timeline Core

### `RHM-G7-01` — Binding 与 Relation Domain Contract

- **目标：** 由 Domain Owner 冻结 Binding、Relation evidence、Inference provenance 和 unresolved lifecycle 的 Core 类型。
- **依赖：** `GATE-G6` 通过。
- **独占路径：** `crates/next-infra-core/src/binding/**`、`crates/next-infra-core/src/relation_evidence/**` 及对应 Core tests。
- **范围：** configured/inferred evidence、confidence、rule/input version、Binding identity/lifecycle 和 Change kinds。
- **非目标：** 不修改 Query DTO/generated TS，不实现 Store、Inference 或 UI。
- **验收：** 同一 endpoints 允许多 evidence；时间邻近不能自动成为因果关系；Binding 不等于资源合并。
- **验证：** Core contract/invariant tests。
- **风险/停止：** 这是 Core 单写契约；QDTO Owner 只能在本任务冻结后投影。

### `RHM-G7-02` — Goal 7 QDTO Contract

- **目标：** 由 QDTO Owner 将 Goal 7 Domain 契约投影为 bounded Topology、Binding 和 Timeline DTO。
- **依赖：** `RHM-G7-01`。
- **独占路径：** `crates/next-infra-query/src/dto/goal7/**`、schema/generator 和 `apps/desktop/src/generated/query/**`。
- **范围：** frontier、truncated、evidence variants、Binding commands/results、timeline groups、cursor 和 version links。
- **非目标：** 不实现 Query algorithms、Store、Adapter 或 UI。
- **验收：** TS generated binding 与 Rust 原子一致；hard-limit metadata、provenance 和 unresolved 状态结构上可表达。
- **验证：** Query DTO/schema tests、binding drift check、TypeScript build。
- **风险/停止：** Domain 变更回派 `RHM-G7-01`；本任务不同时拥有 Core 路径。

### `RHM-G7-03` — Goal 7 SQLite Migration

- **目标：** 由既有 Migration Owner 单写加入 Binding、Relation evidence 和 Inference projection 所需 schema。
- **依赖：** `RHM-G7-01`；schema design 已冻结。
- **独占路径：** `crates/next-infra-store/src/migrations/**` 和 Goal 7 migration tests。
- **范围：** 新 migration、indexes、foreign keys、upgrade/rollback-failure safety 和数据目录自检。
- **非目标：** 不实现 Binding service 或改写已合并 migration。
- **验收：** 从 Goal 6 schema 可重复升级；失败拒绝写入；旧 facts/versions 不丢失；migration 编号唯一。
- **验证：** Store migration upgrade/integrity tests。
- **风险/停止：** `GATE-G7` 只能集成本任务产物，不能自行编辑 migration。

### `RHM-G7-04` — Binding Store 与本地配置服务

- **目标：** 保存人工 Binding 及其生命周期，不改写 Provider facts。
- **依赖：** `RHM-G7-01`、`RHM-G7-03`。
- **独占路径：** `crates/next-infra-binding/**`。
- **范围：** create/update/disable、endpoint validation、unresolved、Change/Audit record。
- **非目标：** 不自动 merge Resource，不执行外部写操作，不编辑 migration 或 QDTO。
- **验收：** endpoint 消失时标记 unresolved 而非静默删除；configured evidence 与 Provider evidence 分离。
- **验证：** Binding unit/integration tests。
- **风险/停止：** schema gap 回派 Migration Owner；DTO gap 回派 QDTO Owner。

### `RHM-G7-05` — Inference Engine 与 Provenance

- **目标：** 以版本化规则产生可解释、可重放的 inferred Relation。
- **依赖：** `RHM-G7-01`；可与 QDTO/Migration 分支并行。
- **独占路径：** `crates/next-infra-inference/**`、合成 rule fixtures。
- **范围：** rule inputs、confidence、rule/input versions、重算和失效。
- **非目标：** 不基于时间相邻猜因果，不自动 merge identity，不编辑 Query DTO。
- **验收：** 相同输入确定性输出；每条 relation 可回到具体 rule/input versions；输入缺失语义明确。
- **验证：** golden/property/replay tests。
- **风险/停止：** Provider crate 不能内置跨 Provider inference。

### `RHM-G7-06` — Bounded Topology Query

- **目标：** 提供 focus-first、有硬上限、可逐层展开的 topology query。
- **依赖：** `RHM-G7-02`；可用 synthetic relations 与 Binding/Inference 并行。
- **独占路径：** `crates/next-infra-query/src/topology/**`。
- **范围：** default depth 1、default 100/200、hard 200/400、stable order、truncated/frontier。
- **非目标：** 无 global graph、客户端 load-all bypass 或 DTO 修改。
- **验收：** 边界在 Query Service 执行；三类 evidence 保留；frontier 可稳定续查。
- **验证：** Topology bound/property tests。
- **风险/停止：** DTO 变化回派 `RHM-G7-02`。

### `RHM-G7-07` — Timeline Query

- **目标：** 提供结构化、分页、无重复轮询噪声的 Change Timeline。
- **依赖：** `RHM-G7-02`；可与其他 Goal 7 分支并行。
- **独占路径：** `crates/next-infra-query/src/timeline/**`。
- **范围：** SyncRun/Binding/Inference grouping、Version links、field diff summary、absolute time。
- **非目标：** 不提供 log terminal、raw payload、infinite history 或 DTO 修改。
- **验收：** 未变化 poll 不出现；每项有 source/version/evidence；cursor stable。
- **验证：** Timeline query tests。
- **风险/停止：** DTO gap 回派 `RHM-G7-02`。

### `RHM-G7-08` — Goal 7 Core 集成证据

- **目标：** 串联 Binding、Inference、Topology 和 Timeline，向 `GATE-G7` 提供可重放证据。
- **依赖：** `RHM-G7-04..07`、`UI-G7-01/02`。
- **独占路径：** `tests/integration/binding_topology_timeline/**`。
- **非目标：** 不修改 Core/Store/Query/Binding/Inference/UI production modules 或 migration。
- **验收：** configured/inferred/provider 不混淆；unresolved 可见；hard limits 有效；Timeline 无重复未变化 poll。
- **验证：** Goal 7 Core/Query/UI acceptance suites。
- **风险/停止：** 集成问题回派 owner；不在 test 中隐式修数据。
