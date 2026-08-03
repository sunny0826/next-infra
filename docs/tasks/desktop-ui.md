# Desktop UI 任务包

本文件覆盖 React/TypeScript Shell、Desktop Adapter、Evidence Spine、六个主页面、响应式与真实 Tauri smoke。视觉和交互权威输入为 [`visualization-and-interaction.md`](../design/visualization-and-interaction.md)、[Interface System](../../.interface-design/system.md) 与 [HTML 原型](../../prototype/README.md)。通用调度规则见[总调度手册](./README.md)。`GATE-G1` 已通过；Goal 1 UI 任务处于复核完成状态，`UI-G2-01` 继续等待 Goal 2 Domain/QDTO 契约。

## 1. UI 所有权与验证矩阵

| Owner | 独占路径 |
| --- | --- |
| QDTO Owner | `crates/next-infra-query/src/dto/**`、schema/generator、`apps/desktop/src/generated/query/**` |
| Host Owner | Tauri lifecycle/tray/single-instance modules；shared entrypoint/config 由 Composition Captain 串行集成 |
| Shell Owner | `apps/desktop/src/main.tsx`、`app/**`、`styles/**`、`ui/**` |
| Adapter Owner | `apps/desktop/src/platform/desktop-adapter/**`、`src-tauri/src/adapter/**` |
| Evidence Owner | `apps/desktop/src/features/evidence/**` |
| Page Owner | 对应的 `features/<page>/**` |
| UI Fixture Owner | `apps/desktop/src/test/fixtures/**` |
| UI QA Owner | `apps/desktop/tests/**`、`e2e/**`；不能直接改生产文件 |

生成的 TypeScript binding 禁止手改。页面需要新增字段时，只能向 QDTO Owner 提交契约需求。

验证组：

- `V-UI`：Desktop lint、component tests、TypeScript build。
- `V-CONTRACT`：Query DTO Rust tests、binding drift check、TypeScript build。
- `V-ADAPTER`：`rtk cargo test -p next-infra-desktop-adapter` + `V-UI`。
- `V-VIEWPORT`：至少 1600×1000、900×800、390×844；最终仍须六页逐页回归。
- `V-DESKTOP`：依次执行 `rtk pnpm --dir apps/desktop tauri build`、`rtk pnpm --dir apps/desktop test:bundle-boundary` 与 `rtk pnpm --dir apps/desktop test:desktop-smoke`；Vite 页面不能替代。

Goal 1 冻结实际 script 名后，用确定命令替换这些验证组；不得在任务执行时各自发明不同脚本。

## 2. Goal 0：UI Readiness

### `UI-G0-01` — Interface Readiness Review

- **状态：** `READY-DESIGN`。
- **目标：** 对齐 Interface System、视觉文档、平台决策和 UI 文件所有权，给出能否进入工程阶段的结论。
- **依赖：** 无。
- **独占路径：** 只读；若发现冲突，只输出缺陷清单，不自行改工程文件。
- **范围：** dark graphite 基线、Critical Path、Inspector、三栏 layout、六页导航、状态语义、responsive/a11y 和测试方案。
- **非目标：** 不创建 Rust/Node/Tauri 文件。
- **输出：** readiness 结论和阻塞项。
- **验收：** 用户明确授权 Goal 1；设计状态与 Interface System 不矛盾；平台未决项均有 owner。
- **验证：** Goal 0 文档检查、链接检查。
- **风险/停止：** “继续设计”不等于编码授权。

## 3. Goal 1：Shell 与契约骨架

### `UI-G1-01` — Rust QDTO → TypeScript Binding Pipeline

- **状态：** `REVIEW`。
- **目标：** 建立 Rust 权威 schema 到只读 TypeScript binding 的单向管线。
- **依赖：** `RHM-G1-01` workspace bootstrap。
- **独占路径：** QDTO/schema/generator/generated paths；由未来 QDTO Owner 执行，不与页面任务并行修改。
- **范围：** schema version、snapshot metadata、pagination、error envelope、最小 Resource/Relation/Connection DTO 和 drift check。
- **非目标：** 不实现 Query Service、页面或 Provider DTO。
- **输入/输出：** glossary/resource model → Rust DTO、generated TS、CI drift failure。
- **验收：** React 无手写同名 schema；Rust 变化未生成时 CI 失败；Secret 字段结构上不可达。
- **验证：** `V-CONTRACT`。
- **实现证据（2026-08-02）：** Rust QDTO、12 个只读 TypeScript binding、确定性导出及 modified/untracked drift guard 已落地；6 个 Rust 契约测试、Clippy、TypeScript build、clean/negative drift 回归均通过。
- **风险/停止：** Rust 定义、生成物和 snapshot 必须由一个 owner 原子提交。

### `UI-G1-02` — React App Shell

- **状态：** `REVIEW`。
- **目标：** 建立符合 Interface System 的三栏桌面应用框架。
- **依赖：** `RHM-G1-01` 提供可启动的 Tauri/Vite host；可与 `UI-G1-01` 并行。
- **独占路径：** Shell Owner paths。
- **范围：** tokens、Navigation、Context Bar、Primary Canvas、Inspector Host、Runtime Bar 和六个占位 routes。
- **非目标：** 不接真实 Query、不实现页面数据或 Tauri 生命周期。
- **输入/输出：** Interface System → 稳定 Shell 与 feature registration points。
- **验收：** 188/316 桌面栏、52/29 上下条、1180/820/560 响应规则按规范生效；六项导航有可访问名称；不退回 KPI/card-grid 仪表盘。
- **验证：** `V-UI + V-VIEWPORT`。
- **实现证据（2026-08-02）：** 六 route Shell、可开闭 Inspector、只读 Context/Runtime bar 与精确响应规则已落地；8 个组件测试及 1600×1000、900×800、390×844 真实浏览器回归通过，三种 viewport 均无页面级横向溢出或控制台错误。
- **风险/停止：** 不复制原型 fixture 文案为产品常量；页面 owner 不编辑 route registry/global CSS。

### `UI-G1-03` — Empty/Mock Desktop Adapter

- **状态：** `REVIEW`。
- **目标：** 确保 SPA 只能通过 `DesktopAdapter` port 获取数据。
- **依赖：** `UI-G1-01`。
- **独占路径：** TypeScript Adapter paths；UI fixture 使用专属目录。
- **范围：** adapter interface、Empty Adapter、Mock Adapter、dependency injection 和 contract tests。
- **非目标：** 不调用 Tauri、不模拟 Provider 写操作、不重定义 DTO 业务语义。
- **输入/输出：** generated bindings → adapter port 与 deterministic mock。
- **验收：** feature code 不直接 `invoke/listen`；browser component tests 不需要 Tauri；mock 无真实身份/凭据。
- **验证：** `V-UI + V-CONTRACT`。
- **实现证据（2026-08-02）：** 只读 port、Empty/Mock Adapter、React DI 与虚构 fixture 已落地，生产 composition 仅注入 Empty Adapter；5 个测试文件共 30 项测试、依赖图守卫、TypeScript build 与 QDTO drift 均通过。
- **风险/停止：** 页面不能私自扩展 Adapter；mock 不成为第二套 Query Service。

### `UI-G1-04` — Tauri Bootstrap Smoke

- **状态：** `REVIEW`。
- **目标：** 证明真实 Tauri App 可以承载 Mock Shell。
- **依赖：** `UI-G1-02/03`、Host skeleton。
- **独占路径：** `apps/desktop/e2e/bootstrap/**`；QA 只提交测试。
- **范围：** 启动、主窗口渲染和退出。
- **非目标：** 不测试托盘、hide/restore 或 Runtime 持续运行。
- **输出：** 最小真实 bundle smoke。
- **验收：** App 实际启动并渲染 Shell；不访问 SQLite、Keychain 或 Provider。
- **验证：** `V-DESKTOP` 的 Goal 1 子集。
- **实现证据（2026-08-03）：** 最新 release bundle 构建成功；`test:desktop-smoke` 验证唯一新 PID `76140`、屏幕内主窗口（900×600）、ScreenCaptureKit 截图 `/tmp/next-infra-goal1-rendered-20260803.png`（91921 bytes）和精确 PID 退出均通过。主 agent 已直接检查截图，确认完整深色 Shell、六项导航、Overview、Goal 1 placeholder、Inspector 控件和 Runtime Bar 可见。此前锁屏会话中的 `SCStreamErrorDomain -3811` 与系统 `screencapture` 失败属于环境归因，macOS 解锁后复验通过。
- **风险/停止：** 签名/环境失败单独归因；QA 不改 Host。

## 4. Goal 2：Query Snapshot Fixture

### `UI-G2-01` — UI Fixture Catalog

- **状态：** `REVIEW`。
- **目标：** 为 Goal 3 页面提供与 QDTO/Domain 一致的确定性 Query snapshots。
- **依赖：** Goal 2 domain/DTO 枚举冻结；与 `CON-G2-03` 的 Observation Fixture 分层。
- **独占路径：** UI Fixture Owner paths。
- **范围：** Resource、Relation、Change、Connection、Connector Coverage、Sync Coverage、SyncRun 和各种视图状态。
- **非目标：** 不连接 Provider，不实现页面；不复制 Connector 内部 Observation fixture。
- **输入/输出：** generated DTO → schema-valid query snapshot catalog。
- **验收：** 包含三种 Evidence、healthy-but-expired、partial、tombstoned、orphaned、unresolved、empty/error/loading；无真实 hostname/repo/IP。
- **验证：** Fixture schema/serialization/determinism tests。
- **实现证据（2026-08-03）：** Rust 权威 QDTO 已增加 Change、Connector Coverage、四类 Sync Coverage、SyncRun、错误、计数和 loading/ready/empty/partial/error 视图状态，并确定性生成 14 个新增 TypeScript binding。UI fixture catalog 覆盖三类 Relation evidence、healthy-but-expired、tombstoned、orphaned、unresolved endpoint、三档 Connector Coverage、完整/增量/targeted/partial SyncRun、failed/interrupted、empty/error/loading；全部使用合成 ID，无真实 hostname/repo/IP/Secret。6 项 fixture 测试、QDTO contract/export、TypeScript lint 与 Desktop 全部测试通过。
- **风险/停止：** Query snapshot 与 Store/Query 语义漂移时回派 QDTO/Query owner。

## 5. Goal 3：Desktop UI 纵切

### `UI-G3-01` — Thin Tauri Desktop Adapter

- **状态：** `REVIEW`。
- **目标：** 把 React 请求映射到 Query Service 或明确的本地配置服务。
- **依赖：** `RHM-G3-01` QDTO/Query、Host registration point。
- **独占路径：** Adapter Owner paths；Host Owner 只负责最终注册。
- **范围：** bounded Command wrappers、clean errors、Manual Sync enqueue、invalidation-only events 和 re-query coordination。
- **非目标：** 无 SQL、Provider SDK、状态推导、Keychain 通用读取或完整 Event payload。
- **输入/输出：** Query/local-config services → TS Real Adapter 与 Rust Command/Event adapter。
- **验收：** Event 只含 version/minimal scope；丢失/乱序 Event 不破坏状态；Manual Sync 返回 `sync_run_id`；页面无直接 Tauri API。
- **验证：** `V-ADAPTER`。
- **实现证据（2026-08-03）：** TypeScript `DesktopAdapter` 已冻结为八个共享 Query 入口、Manual Sync、local settings/capabilities 和 invalidation subscription；Connection list 使用带 metadata、硬上限 200 的 `ConnectionSnapshotDto`。Goal 1 遗留且无 Rust Query 对应入口的 metadata/resource/relation list methods 已删除，Empty Adapter 不再把 Host unavailable 伪装成 empty snapshot。`RealDesktopAdapter` 仅通过稳定 command/event 名称调用 Tauri，未知平台错误清洗为安全 envelope，Manual Sync 只返回 `sync_run_id`，Event 只承载 version/scopes。Rust `DesktopQueryAdapter` 只映射共享 Query Service，并提供 ManualSyncPort 与最小 QueryInvalidation DTO；未接 SQL、Provider、Keychain 或 Tauri entrypoint。Desktop Rust 23 项测试、前端 67 项测试、严格 Clippy、lint 和 build 通过。真实 command registration/event emission 由 `RHM-G3-05` Composition Captain 串行接入。
- **风险/停止：** Command 不复制 Query 规则；Event 不是权威状态。

### `UI-G3-02` — Evidence Spine

- **状态：** `REVIEW`。
- **目标：** 实现全页面共用的 Current Facts + Evidence Path Inspector。
- **依赖：** QDTO、Mock Adapter；可与 Adapter wiring 并行使用 mock。
- **独占路径：** Evidence Owner paths。
- **范围：** provider/configured/inferred variants、desktop inspector 和 narrow drawer。
- **非目标：** 不自行查询、推断 evidence 或持有页面权威状态。
- **输入/输出：** Evidence DTO/view model → composable Inspector。
- **验收：** configured 不伪造 SyncRun；inferred 显示 rule/input versions/confidence；相同 endpoints 的多 evidence 不丢失。
- **验证：** `V-UI + V-VIEWPORT`。
- **实现证据（2026-08-03）：** 已实现 Current Facts 与垂直 Evidence Path；Current Facts 分别显示 Health/Freshness/Lifecycle，provider evidence 显示 connector type/connection/sync run/field path，configured evidence 显示 Binding 与 created_at 且不伪造 SyncRun，inferred evidence 显示 rule、ResourceVersion/RelationVersion inputs 与 confidence；相同 endpoints 的多条 evidence 保留。4 项 focused component tests、Desktop 全部 40 项测试、lint 与 build 通过；最终三 viewport drawer 回归留给 `UI-G3-08`。
- **风险/停止：** 页面不得复制 Spine；provenance 缺失时不在展示层猜测。

### `UI-G3-03` — Overview

- **状态：** `REVIEW`。
- **目标：** 完成 Attention → Observation → Critical Path → Changes 核实路径。
- **依赖：** `UI-G2-01`、`UI-G3-01/02`。
- **独占路径：** `apps/desktop/src/features/overview/**`。
- **范围：** Query/fixture snapshot、选择打开 Evidence Spine、清晰异常优先级。
- **非目标：** 无 KPI card grid、自动猜 Critical Path 或外部操作。
- **验收：** 区分 Resource unhealthy、expired 和 Connector failure；默认聚焦最重要异常；始终显示来源/观察时间。
- **验证：** `V-UI + V-VIEWPORT`。
- **实现证据（2026-08-03）：** Overview feature 已实现 Attention Queue、Observation Strip、configured-only Critical Paths 和 Recent Changes 四段核实路径；Attention 明确分离 Resource Health、Freshness 与 Lifecycle，Observation 单列 Connector Health/last success/last attempt，未配置 Critical Path 时明确拒绝根据名称或活动猜测。页面始终显示来源上下文与 observed_at，并通过 callback 交给后续 Inspector。3 项页面测试及 Desktop 全部 47 项测试、lint/build 通过；Shell 接入与三 viewport 回归留 `UI-G3-07/08`。
- **风险/停止：** Observation succeeded 不等于 Resource healthy。

### `UI-G3-04` — Inventory 与 Resource Detail

- **状态：** `REVIEW`。
- **目标：** 完成资源发现和单资源核实路径。
- **依赖：** `UI-G2-01`、`UI-G3-01/02`；可与其他页面并行。
- **独占路径：** `features/inventory/**`、`features/resource-detail/**`。
- **范围：** filter、stable sort、pagination、row selection、identity/status/evidence/relations/attributes/change/coverage。
- **非目标：** 不显示 raw Provider JSON，不一次渲染全部数据。
- **验收：** 单次最多 100；Health/Freshness/Lifecycle 分离；empty/error/partial 状态不混合。
- **验证：** `V-UI + V-VIEWPORT`。
- **实现证据（2026-08-03）：** Inventory 已实现 bounded search、attention filter、stable name sort、opaque cursor 分页、键盘 Enter/Space 行选择和内部横向滚动；固定列分别展示 Health、Freshness、Lifecycle、Connection 与 observed_at，默认请求 25 且 UI 明示单次上限 100。Resource Detail 已实现 identity、Health/Freshness/Lifecycle/observed_at、Evidence Spine、normalized scalar attributes、Recent Changes 与 Connector Coverage 连续核实路径；对关系端点执行 bounded detail query，并按相同 endpoints 保留 provider/configured/inferred 多 evidence，不输出 raw Provider JSON。Inventory + Detail 共 5 项页面测试，Desktop 全部 52 项测试、lint/build 通过；Shell 与 viewport 验收留 `UI-G3-07/08`。
- **风险/停止：** 前端不重算 Freshness；opaque cursor 不可编辑。

### `UI-G3-05` — Minimum Bounded Topology

- **状态：** `REVIEW`。
- **目标：** 交付 Fixture 驱动的 focus-first topology。
- **依赖：** `UI-G2-01`、`UI-G3-01/02`；可与其他页面并行。
- **独占路径：** `features/topology/**`。
- **范围：** focus、depth 1、100/200 defaults、200/400 hard limits、Frontier、三种线型、node/edge inspector。
- **非目标：** 无 global graph、Binding edit 或最终布局优化。
- **验收：** 不提供 load-all；truncated/frontier 清晰；三种 evidence 不只靠颜色区分。
- **验证：** `V-UI + V-VIEWPORT`。
- **实现证据（2026-08-03）：** Topology feature 已固定 focus-first depth 1、请求默认 100 nodes/200 edges 并在 toolbar 明示 200/400 hard limits；节点为 136×64，画布与窄屏只在自身容器滚动。provider/configured/inferred 分别使用实线、中心间距 4px 双线、6/5 虚线，并始终显示文字 legend/edge label；node/edge 可键盘聚焦并交给 Inspector callback。Frontier 提供按 resource 继续 bounded query 的入口，不存在 load-all。3 项页面测试及 Desktop 全部 55 项测试、lint/build 通过；最终 viewport/layout smoke 留 `UI-G3-07/08`。
- **风险/停止：** 客户端不能绕过 server hard limits；大图 layout 不阻塞主线程。

### `UI-G3-06` — Connectors 与 Settings 最小页

- **状态：** `REVIEW`。
- **目标：** 展示 Fixture Connection 状态和本地生命周期设置。
- **依赖：** Adapter local-config methods、Host capability DTO；可与其他页面并行。
- **独占路径：** `features/connectors/**`、`features/settings/**`。
- **范围：** Connection/最近同步/退避；start-at-login、data budget、retention、`user_quit`；unsupported capability 显式禁用。
- **非目标：** 不实现 Goal 9 Coverage Matrix，不显示 Secret，不伪装 MCP auto-launch 已可用。
- **验收：** Manual Sync 与 UI refresh 分离；start-at-login 与 MCP auto-launch 分离；Runtime Bar 不替代 Resource Health/Freshness。
- **验证：** `V-UI + V-ADAPTER`。
- **实现证据（2026-08-03）：** Connectors 已分开展示 Connection Health、last success/attempt、recent SyncRun、next scheduled 与 Connector Coverage，Manual Sync 只显示新 `sync_run_id` 并明确当前页面未刷新；disabled connection 不可触发。Settings 以连续 row 分开 start-at-login、MCP auto-launch、`user_quit`、data budget 与 retention；unsupported MCP capability 显式禁用，页面声明并测试 Secret/SecretRef 不进入 DOM。4 项页面测试及 Desktop 全部 59 项测试、lint/build 通过；真实 local-config/autostart wiring 留 Composition。
- **风险/停止：** SecretRef 不进入 DOM；无 capability 的开关不能看似可用。

### `UI-G3-07` — Shell Integration

- **状态：** `REVIEW`。
- **目标：** 由唯一 Shell Owner 接入所有已完成 feature。
- **依赖：** `UI-G3-02..06` 通过各自 tests。
- **独占路径：** Shell Owner route registry、context/search/inspector host。
- **范围：** routes、current scope、global search、window restore re-query。
- **非目标：** 不修改 feature 内部实现。
- **验收：** Overview、Inventory、Resource Detail、Topology 可核实；Timeline 明确标为 Goal 7 未完成，而非伪装 empty。
- **验证：** `V-UI`。
- **实现证据（2026-08-03）：** Shell 已接入六个 route、bounded global search、route-scoped selection/detail/topology state、Resource/Relation Inspector、window focus/invalidation re-query 和 Tauri-only Real Adapter composition。Search/Inventory 选择可进入 Detail 并成为可达的 bounded Topology focus；Inspector 按对象身份、Current Facts、Evidence 展示实际 provider/configured/inferred provenance；Timeline 显式标为 Goal 7 unavailable。异步 subscription cleanup 可处理 unmount 后晚到的 unsubscribe。Shell CSS 补齐 dropdown、feature wrapper、Inspector facts、1180/820/560 drawer/overflow 规则，未引入新色、阴影或渐变。Desktop 14 个测试文件共 63 项测试、lint/build、binding drift、Desktop Rust 23 项测试与严格 Clippy、Rust workspace 104 项测试与严格 Clippy全部通过；真实三 viewport 留 `UI-G3-08`。
- **风险/停止：** 页面 worker 不与本任务同时编辑 registry/global shell。

### `UI-G3-08` — Responsive 与 Accessibility QA

- **状态：** `REVIEW`。
- **目标：** 验证 Goal 3 代表路径的响应式和可访问性。
- **依赖：** `UI-G3-07`。
- **独占路径：** `tests/responsive/**`、`tests/accessibility/**`。
- **范围：** 三 viewport、focus、Command/Ctrl+K、Escape、Enter/Space、reduced motion、container scroll。
- **非目标：** QA 不直接修 feature；Topology arrow navigation 留 Goal 7。
- **输出：** automated regressions 和按 owner 路由的 defects。
- **验收：** 无 document-level 横向 overflow；状态均有文本；代表核实路径通过。
- **验证：** `V-VIEWPORT`。
- **实现证据（2026-08-03）：** QA-only Vite fixture 复用正式 Shell/CSS 与合成 Query snapshot，未给生产 `main.tsx` 增加 debug 分支。Playwright CLI 真实验证 1600×1000、900×800、390×844：HTML/body scrollWidth 均等于 viewport；桌面 grid 为 188/1096/316，900px 为 188/712，390px 为 56/334；900px 与 390px Inspector fixed drawer 均在 viewport 内并保留 close button。Inventory 670/261、Connectors 660/261、Topology 760/261 只在内部容器滚动。真实浏览器验证 Meta+K、2px cyan focus、reduced-motion、Settings disabled capability、Timeline unavailable 与 SecretRef 不进 DOM；干净 session 无 console error。新增 accessibility regression 后 Desktop 16 个测试文件共 67 项测试、lint/build 与 diff check 通过。真实 Tauri 窗口行为仍留 `UI-G3-09`。
- **风险/停止：** 代表 viewport 通过不等于最终六页通过。

### `UI-G3-09` — Real Desktop Lifecycle QA

- **目标：** 从用户可见路径验证 UI 与真实 Desktop Host 生命周期。
- **依赖：** `RHM-G3-05` composition、`UI-G3-07/08`。
- **独占路径：** `e2e/desktop/**`。
- **范围：** close→hide、Runtime continue、restore re-query、second instance activation、explicit quit。
- **非目标：** QA 不修改 Host 或 page implementation。
- **验收：** UI 恢复不依赖旧 React state；真实 bundle 行为符合 Goal 3；只有一个 DB owner。
- **验证：** `V-DESKTOP`。
- **风险/停止：** 浏览器测试不能替代 App；失败按 Host/Adapter/Page 分层归因。

Goal 3 页面并发拓扑：

```text
RHM-G3-01 + UI-G2-01
      ├─ UI-G3-01 Adapter
      └─ UI-G3-02 Evidence Spine
                 ↓
      ┌──────────┼───────────┬───────────┐
 UI-G3-03    UI-G3-04    UI-G3-05    UI-G3-06
      └──────────┴───────────┴───────────┘
                       ↓
                   UI-G3-07
                    ├───────┐
                UI-G3-08  UI-G3-09
```

## 6. Goal 4–9 的 UI 验收包

### `UI-G4-01` — Host/MCP State UI

- **目标：** 接入 MCP auto-launch capability、`host_unavailable` 和 `user_quit` 状态。
- **依赖：** Goal 4 Host/Bridge DTO。
- **独占路径：** Settings Host/MCP state module；Adapter 变化由 Adapter Owner单独提交。
- **非目标：** 不实现 Bridge，不允许 UI/Agent 越权清除 suppression。
- **验收：** auto-launch 和 start-at-login 分开；disabled reason/处理指引明确；MCP unavailable 不显示为资源 empty。
- **验证：** `V-UI + V-DESKTOP`。
- **风险/停止：** capability 缺失时保持禁用，不伪装成功。

### `UI-G5-01` — GitHub 纵切 UI Acceptance

- **目标：** 在既有页面核实 Repo → Workflow → Run/Deployment。
- **依赖：** `CON-G5-02`、`CON-G5-03` 的清洗后 Query snapshots；不依赖尚未执行的 `CON-G5-04` 纵切汇合任务。
- **独占路径：** `tests/acceptance/github/**`。
- **非目标：** 不创建 Provider 专属页面。
- **验收：** 429、permission 和 partial 不触发误删或假 empty；来源/时间/evidence 可见。
- **验证：** `V-UI + V-DESKTOP`。
- **风险/停止：** fixtures 不包含私有 repo data。

### `UI-G6-01` — SSH / Mac mini UI Acceptance

- **目标：** 核实 SSH Host 的 Freshness 与 Connector Health 语义。
- **依赖：** `CON-G6-02..04` 的清洗后 Query snapshots；不依赖尚未执行的 `CON-G6-05` 纵切汇合任务。
- **独占路径：** `tests/acceptance/ssh/**`。
- **非目标：** 无 command input 或 arbitrary probe UI。
- **验收：** unreachable 影响 Freshness/Connector Health，不伪造 Resource down；Host Key mismatch 明确。
- **验证：** `V-UI + V-DESKTOP`。
- **风险/停止：** fixture 无真实 alias/IP。

### `UI-G7-01` — Topology Binding 与键盘导航

- **目标：** 在最小 Topology 上完成 Binding、逐层展开和可访问图导航。
- **依赖：** `RHM-G7-01/02/04/06` 发布 Domain、QDTO、Binding 和 bounded Topology Query。
- **独占路径：** Topology Owner paths。
- **范围：** grouping、frontier expansion、arrow-key adjacency、configured Binding、unresolved placeholder。
- **非目标：** 不自动 merge resources，不绕过 Adapter 写本地 Binding。
- **验收：** Binding source 清晰；endpoint 失效不静默删除；hard limits 始终有效。
- **验证：** `V-UI + V-VIEWPORT + V-ADAPTER`。
- **风险/停止：** 页面不能直接改 Relation state；configured evidence 不标为 Provider。

### `UI-G7-02` — Timeline

- **目标：** 将 Timeline 占位 route 升级为正式核实页面。
- **依赖：** `RHM-G7-07` Timeline Query、Evidence Spine。
- **独占路径：** `features/timeline/**`；Shell Owner负责最终接入。
- **范围：** SyncRun/Binding/Inference groups、field diffs、Version links、local/absolute time、pagination。
- **非目标：** 无 log terminal、unchanged poll、raw payload 或 infinite history。
- **验收：** 620px 内部滚动为硬约束；大 before/after fields 默认折叠；每项可追溯。
- **验证：** `V-UI + V-VIEWPORT`。
- **风险/停止：** timezone/version chain 漂移回派 Query owner。

### `UI-G8-01` — Dokploy/Cloudflare 端到端 Topology Acceptance

- **目标：** 核实 Repo → Deployment → Host → DNS 代表链。
- **依赖：** `CON-G8-02`、`CON-G8-04` 和 Goal 7 Topology；不依赖尚未执行的 `CON-G8-05` replay gate。
- **独占路径：** `tests/acceptance/dokploy-cloudflare/**`。
- **非目标：** 不增加 Provider 专属配色/图，不根据 display name 猜 relation。
- **验收：** 每条 relation 的 provider/configured/inferred source 可见；bounded query 可重放。
- **验证：** `V-UI + V-DESKTOP`。
- **风险/停止：** UI 不补做 Connector/Inference 缺失的关系。

### `UI-G9-01` — Connector Coverage Matrix

- **目标：** 完成 Connectors 页逐 module Coverage Matrix。
- **依赖：** `CON-G9-04` Connector Coverage Snapshot。
- **独占路径：** Connectors page coverage module。
- **范围：** Supabase managed/self-hosted、Aliyun/Tencent 各产品的 supported/partial/unsupported、reason 和 auth minimum。
- **非目标：** 不使用“支持整个云厂商”的 summary，不从 SyncRun 反推支持状态。
- **验收：** Connector Coverage、Sync Coverage、Connection Health 三者独立展示。
- **验证：** `V-UI + V-VIEWPORT`。
- **风险/停止：** schema gap 回派 Catalog/QDTO owner，不在 UI 猜测。

### `UI-G9-02` — 六页最终回归

- **目标：** 对 Overview、Inventory、Topology、Timeline、Connectors、Settings 逐页执行最终验收。
- **依赖：** `UI-G9-01` 和 Goal 0–9 全部 UI/Connector/Core 分支。
- **独占路径：** `tests/regression/six-pages/**`、`e2e/six-pages/**`。
- **范围：** all states、三 viewport、keyboard、reduced motion、Evidence Spine、代表路径和真实 Desktop。
- **非目标：** QA 不直接修生产文件。
- **验收：** 六页逐页通过；Overview → Topology → Resource/Relation Inspector → Timeline 完整；没有 document overflow、无界查询或 secret DOM leak。
- **验证：** 全部 `V-*`。
- **风险/停止：** Connector 环境失败与 UI regression 分开报告。
