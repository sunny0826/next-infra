# Next Infra 术语表

**状态：** Draft（规范性术语基线）  
**适用范围：** 设计文档、未来 Rust/TypeScript 命名、UI 文案、MCP Schema 和测试描述

本文定义 Next Infra 中每个核心词的唯一含义。英文术语是代码、Schema 和跨文档引用的规范名称；中文用于解释，不另造同义架构名。

## 1. 使用规则

1. 同一组件在架构、UI、MCP 和代码中使用同一个英文术语。
2. 首次出现时可以写成“中文（English Term）”，之后优先使用英文规范名。
3. `Provider`、`Connector`、`Connection` 三者不可互换。
4. `Resource Health`、`Connector Health` 与 `Freshness` 必须分开表达。
5. “支持某 Provider”必须展开成可验证的 `Coverage`，不能表示整个平台全部能力。
6. 首版的“管理”仅表示 inventory、query、visualize 和 local configuration；不表示修改外部资源。

## 2. 架构与进程术语

| 规范术语 | 中文说明 | 精确定义 | 不要混淆 |
| --- | --- | --- | --- |
| Next Infra | 产品名 | 当前用户在本机运行的个人基础设施查看与关系分析工具 | 不是云端控制平面或团队 SaaS |
| Desktop Host | 桌面宿主 | `Next Infra.app` 的唯一长生命周期应用实例；负责 Tauri 生命周期、窗口、托盘、单实例和承载 Runtime | 不是 Domain Core，也不称作 daemon |
| Desktop UI | 桌面界面 | App Bundle 中运行于 WebView 的 React/TypeScript UI | 不是浏览器版产品，也不能直接访问系统能力 |
| Control Plane Runtime | 控制平面运行时 | 不依赖 Tauri 的 Rust 运行时；组合调度、同步、查询、存储、本地 RPC 和维护能力 | 不是独立常驻 daemon；首版由 Desktop Host 承载 |
| Desktop Adapter | 桌面适配层 | 薄 Tauri Commands 与 invalidation-only Events；在 WebView 和 Application Services 之间转换 DTO | 不是 Query Service，不包含 SQL 或 Provider 规则 |
| Query Service | 查询服务 | Desktop 与 MCP 共用的 Rust 查询语义入口；统一字段过滤、分页、Topology 边界、Freshness 和错误清洗 | 不是 HTTP API，也不等于 SQLite Repository |
| Local RPC | 本地 RPC | MCP Bridge 与 Desktop Host 之间的版本化 Unix Domain Socket 协议 | 不是远程 MCP、localhost HTTP 或公开网络接口 |
| MCP Bridge | MCP 桥接进程 | 独立 `next-infra-mcp` 短生命周期 STDIO 进程；把 MCP 请求转换为 Local RPC | 不是 Runtime、Connector 或数据库 owner |
| Scheduler | 调度器 | 按 Connection 周期、抖动、退避、startup/catch-up 规则发起 SyncRun | 不直接解释 Provider 响应 |
| Sync Engine | 同步引擎 | 编排 Connector、Normalizer、Writer、Coverage 和 SyncRun 生命周期 | 不等于某个 Connector |
| Normalizer | 规范化器 | 对 ObservationBatch 做字段白名单、Schema 验证、Fingerprint 和敏感字段检查 | 不是通用原始 JSON 存储 |
| Writer | 单写入者 | 唯一串行化 SQLite 业务写事务的 Runtime 组件 | 不是 Connector 内部数据库连接 |
| Store | 存储适配器 | SQLite 投影、历史、migration、查询与受控维护的 Rust adapter | SQLite 是实现；Store 是端口实现 |
| SecretProvider | 秘密提供器 | 通过 SecretRef 临时访问 Keychain item 的 Rust 端口与实现 | 不向 React、MCP 或日志返回 Secret 值 |

## 3. Provider 接入术语

| 规范术语 | 中文说明 | 精确定义 | 示例或边界 |
| --- | --- | --- | --- |
| Provider | 外部事实来源 | 拥有资源与状态的外部系统或访问面 | GitHub、Cloudflare、Dokploy、SSH Host |
| Connector | 连接器类型 | 与主程序一起编译的只读 Provider adapter；声明认证、Schema、同步模式、限流和 Coverage | `github`、`ssh`、`dokploy` |
| ReadConnector | 只读连接器契约 | 首版 Connector 实现的概念接口；只能观察外部事实 | 不包含 `execute` 或任意 shell command |
| Connection | 数据源实例 | 某个 Connector 的一份本地配置和凭据引用 | 一个 GitHub Account、一套 Dokploy、一组 SSH aliases |
| ConnectorDescriptor | 连接器描述 | Connector 对类型、版本、资源 kind、认证、同步模式、限流和已知缺口的声明 | 用于 Coverage 和契约测试 |
| Observation | 观察 | Connector 在某一时间从 Provider 读取到的一项候选事实 | 尚未必是已提交 ResourceVersion |
| ObservationBatch | 观察批次 | Connector 一次同步返回的 resources、relations、Coverage、cursor、warnings 和清洗摘要 | 不能直接写 SQLite |
| Coverage | 覆盖的总称 | 描述“系统能观察什么”及“本次实际观察了什么” | 上下文可能混淆时必须进一步写明 Sync Coverage 或 Connector Coverage |
| Sync Coverage | 同步覆盖 | 描述某次 ObservationBatch 实际观察的完整程度 | `authoritative_full`、`incremental`、`partial`、`targeted` |
| Connector Coverage | 连接器能力覆盖 | ConnectorDescriptor 声明当前实现对 Provider 各资源模块、字段和关系的覆盖 | `supported | partial | unsupported`，并附已知缺口 |
| authoritative_full | 权威全量覆盖 | 成功完整枚举指定 scope，可为缺失资源增加缺失证据 | 仍需连续两次缺失才 tombstone |
| incremental | 增量覆盖 | 只观察 cursor 之后的变化，不能证明未返回资源不存在 | 不影响缺失计数 |
| partial | 部分覆盖 | 因权限、分页、限流或错误只获得部分事实 | 结果可用但必须显示缺口 |
| targeted | 定向覆盖 | 只刷新指定资源 | 不证明其他资源不存在 |

## 4. 领域模型术语

| 规范术语 | 中文说明 | 精确定义 | 身份或来源 |
| --- | --- | --- | --- |
| Resource | 资源当前投影 | 某 Connection 下一个外部对象的最新已提交规范化事实 | 唯一键 `(connection_id, kind, external_id)` |
| ResourceVersion | 资源版本 | Resource 发生语义变化时创建的不可变快照 | 追溯到 SyncRun；未变化轮询不创建 |
| Relation | 关系当前投影 | 两个 Resource 之间一条带类型和证据的有向关系 | 稳定键包含端点、kind、evidence type/key |
| RelationVersion | 关系版本 | Relation 的端点、类型、证据、confidence 或 lifecycle 变化时创建的不可变快照 | 来源是 SyncRun、Binding 或 Inference |
| Binding | 人工绑定 | 用户明确声明的本地跨平台关系 | 只创建本地 Relation，不修改 Provider |
| Evidence | 关系证据 | 解释 Relation 为什么成立的来源 | `provider`、`configured`、`inferred` |
| provider evidence | Provider 证据 | Provider 明确返回的关系 | 引用 Connection、SyncRun 和字段路径 |
| configured evidence | 配置证据 | Binding 创建的关系 | 引用 Binding，不伪造 SyncRun |
| inferred evidence | 推断证据 | 可解释规则从稳定输入版本计算出的关系 | 引用规则版本、输入 ResourceVersion 和 confidence |
| Evidence Spine | 证据脉络 | UI 中串联 Provider/Connection、SyncRun、Resource、Relation/Change 的规范视觉结构 | 是界面模式，不是数据库实体 |
| SyncRun | 同步运行 | 一次 Connection 同步尝试及其 mode、trigger、Coverage、cursor、计数和结果 | 状态见第 6 节 |
| Change | 结构化变化 | 相邻 ResourceVersion 或 RelationVersion 的已清洗 before/after 差异 | 来源指向 SyncRun、Binding 或 Inference |
| Capability | 能力声明 | Resource 理论上支持的 inspect 能力；未来写能力由独立 Action Connector 声明 | 首版只有只读 inspect 能力 |
| Fingerprint | 语义指纹 | 对排序稳定、已规范化且已清洗内容计算的摘要 | 用于判断是否创建新 Version |
| Projection | 当前投影 | 从已提交观察和本地 Binding 计算出的最新 Resource/Relation 状态 | SQLite 是本地物化视图，不是外部事实源 |
| Snapshot | 查询快照 | Query Service 返回的一次有版本、时间和边界的已提交结果 | 不等于实时 Provider 状态 |
| SecretRef | 秘密引用 | SQLite 中指向 Keychain item 的非秘密内部引用 | 不是密文，也不能由 MCP 读取 Secret |

## 5. 状态与时间术语

| 规范术语 | 回答的问题 | 规范值或规则 |
| --- | --- | --- |
| Resource Health | Provider 最后一次观察时，外部资源报告什么状态 | `healthy | degraded | unhealthy | unknown` |
| Connector Health | 当前 Connection 是否还能继续读取 Provider | `healthy | degraded | auth_failed | rate_limited | unreachable | disabled`；详细失败另有错误分类 |
| Freshness | 已保存观察是否仍足以代表当前事实 | `fresh | stale | expired`，由计划间隔与 `last_seen_at` 计算 |
| Lifecycle | 本地当前投影是否仍被视为存在 | Resource：`active | tombstoned | orphaned` |
| observed_at | 事实观察时间 | Provider 响应或探针事实对应的时间；必须随状态展示 |
| last_seen_at | 最后一次成功看到该事实的时间 | 内容未变化时仍更新 |
| last_changed_at | 当前投影最后发生语义变化的时间 | 只在 Fingerprint 变化时更新 |
| tombstoned | 墓碑状态 | 连续两次未出现在成功且同 scope 的 authoritative full 中 | 不等于已调用 Provider 删除 API |
| orphaned | 孤立状态 | Connection 已被本地删除或停用，但历史 Resource 暂时保留 | 不等于外部资源异常 |
| unresolved | 未解析状态 | Binding 的一个端点当前不可用，Binding 仍保留 | 不静默删除人工关系 |

Resource Health、Connector Health 与 Freshness 可以同时出现。例如：

```text
Resource Health: healthy at last observation
Freshness: expired
Connector Health: auth_failed
```

这三行不是冲突，而是分别回答“上次看到什么”“数据多旧”“现在能否继续读取”。

## 6. Run、错误与生命周期术语

### 6.1 SyncRun status

| 值 | 含义 |
| --- | --- |
| `running` | 已创建但尚未结束 |
| `succeeded` | 在声明的 Coverage 下完整成功 |
| `partial` | 返回了可用观察，但 Coverage 不完整 |
| `failed` | 未形成可提交的有效结果 |
| `cancelled` | 被当前 Runtime 有意取消 |
| `interrupted` | 上一次 Host 异常结束后，在恢复时识别出的遗留 running run |

`partial` 可能出现在不同字段中，Schema 必须保留限定名：完成一次增量同步可以是 `status=succeeded, coverage=incremental`；分页中断并提交已有结果通常是 `status=partial, coverage=partial(...)`；Connector Coverage 中的 `partial` 又表示实现只覆盖某个 Provider 模块的一部分。

### 6.2 重要结构化错误

| 错误 | 含义 | UI/MCP 行为 |
| --- | --- | --- |
| `host_unavailable` | Desktop Host 未运行、未授权自动拉起、启动失败、超时或被 `user_quit` 抑制 | 不伪装成空结果，返回无秘密处理指引 |
| `credential_unavailable` | Keychain 暂不可用或需要用户处理 | 不循环弹窗，不退化为错误 Provider 密码 |
| `authentication_failed` | Provider 明确拒绝当前凭据 | 停止高频重试，提示最小权限检查 |
| `permission_denied` | 已认证但权限范围不足 | 标记 Coverage 缺口，不误 tombstone |
| `rate_limited` | Provider 限流 | 遵守 reset/backoff，不制造 API 风暴 |
| `host_key_mismatch` | SSH Host Key 与信任记录不符 | 立即失败，不自动接受新 key |

### 6.3 `user_quit`

`user_quit` 是 Desktop Host 显式退出前写入的本地自动拉起抑制状态：

- 跨后续 MCP Bridge 进程生效。
- MCP 参数和 Agent 无权清除。
- 只有用户主动启动 App，或已经启用的下一次登录自动启动可以清除。
- 崩溃、操作系统关机和升级重启不写入。

它不是认证 Token、进程锁或数据库字段的替代品。

## 7. 接口术语

| 规范术语 | 精确定义 | 约束 |
| --- | --- | --- |
| Tauri Command | React 请求 Rust 执行一个有界查询或明确本地配置用例 | 输入输出 DTO 版本化；不能是通用 SQL/Shell/HTTP proxy |
| Invalidation Event | Rust 通知 UI 某类查询结果已失效的低频 Tauri Event | 只含版本或最小 scope；不是权威状态载荷 |
| Local Configuration | 修改 Connection、保留期、自动启动等只存在本机的配置 | 不修改任何外部 Provider Resource |
| Manual Sync | 用户要求某个 Connection 立即执行一次只读同步 | 返回 SyncRun；不是外部写操作，也不属于首版 MCP Tool |
| Action | 未来对外部 Resource 的显式写操作 | 首版不存在；必须经过独立 Action RFC、Plan、Approval、Execute 和 Verify |
| MCP Tool | Agent 调用的有界只读函数 | 首版七个工具，不提供 refresh、Secret 或外部写操作 |
| MCP Resource | 使用 `infra://` URI 读取的受限资源表示 | 不能绕过 Query Service 的字段过滤和上限 |
| DTO | adapter 与 service 之间的版本化数据契约 | Rust 语义为权威，TypeScript binding 生成或校验 |
| Schema Version | 某类配置、属性、协议或 DTO 的显式兼容版本 | 不兼容时拒绝猜测字段语义 |
| Cursor | 分页或增量同步的继续位置 | 只在事务成功后前移 |
| Pagination Cursor | 查询下一页的 opaque cursor | 不能被当成 Provider credential |
| Sync Cursor | Provider 增量读取位置 | 与 SyncRun completion 同事务提交 |
| Frontier | 有界 Topology 被截断后可继续展开的边界节点集合 | 配合 `truncated: true` 返回 |

## 8. UI 与可视化术语

| 规范术语 | 精确定义 | 不是什么 |
| --- | --- | --- |
| Overview | 以 Attention Queue、Observation Strip、关键链路和最近 Change 为主的首页 | 不是四张大数字 KPI 卡片 |
| Inventory | 可搜索、过滤、分页的 Resource 当前投影清单 | 不是 Provider 原始对象转储 |
| Topology | 以选中 Resource 为中心、有 depth 和节点/边硬上限的 Relation 图 | 不是一次加载全局资源的 hairball |
| Resource Detail | 汇总身份、Health、Freshness、Evidence、属性、关系和版本的核实页面 | 不是未经清洗的 Provider JSON 页面 |
| Timeline | 按 SyncRun、Binding 或 Inference 来源组织的结构化 Change 序列 | 不是完整日志终端 |
| Connectors Page | 展示 Connection Health、SyncRun、Connector Coverage、权限和错误的页面 | 不代表 Connector 类型本身 |
| Inspector | 选中 Resource、Relation、Change 或 Connection 后出现的上下文检查区 | 不是新的权威数据源 |
| Attention Queue | 按决策价值排列的异常、expired、partial 与连续同步失败列表 | 不是按数量排序的告警墙 |
| Observation Strip | 展示各 Connection 最近成功、下一次计划和覆盖缺口的紧凑区域 | 不是 Resource Health 汇总 |
| Critical Path | 用户明确固定的一小段关键资源关系链 | 不由系统根据名称自动猜测 |

## 9. 容易混淆的词

| 避免单独使用 | 原因 | 应改用 |
| --- | --- | --- |
| backend | 无法说明是 Runtime、Query、Store 还是 Provider | 使用具体组件名 |
| daemon | 当前架构没有独立 Next Infra daemon | `Desktop Host` 或 `Control Plane Runtime` |
| server | 容易暗示公开网络服务 | `Desktop Host`、`MCP Bridge`、`Local RPC Server` |
| API | 可能指 Provider、Tauri、Query 或 MCP | `Provider API`、`Tauri Command`、`Query Service`、`MCP Tool` |
| real-time | 首版采用轮询，无法保证实时 | `observed_at`、`Freshness`、`poll interval` |
| offline | 可能是资源异常、数据过期或 Connector 失败 | 分别使用 Health、Freshness、Connector Health |
| supported | 无法表达 Provider 内部覆盖差异 | 按模块列出 `Connector Coverage: supported/partial/unsupported` |
| resource deleted | 可能误解为执行了外部删除 | `tombstoned` 或 `orphaned` |
| operation | 首版不具备外部写操作 | `query`、`local configuration`；未来使用 `Action` |
| approval | 容易退化成布尔值 | 未来使用选项式 `allow_once / allow_for_session / reject / edit_plan` |

## 10. 命名规范

- 资源 kind 使用小写 namespace：`github.repository`、`cloudflare.dns_record`。
- 内部 ID 使用 `<entity>_id`：`resource_id`、`connection_id`、`sync_run_id`。
- 时间字段使用 UTC 存储、界面本地化展示，并保留可复制绝对时间。
- Rust/TypeScript 枚举值使用文档中的小写稳定值，不在 UI 文案中另造同义状态。
- `Host` 单独出现时只指 Desktop Host；`Runtime` 单独出现时只指 Control Plane Runtime。
- `Connector` 指类型，`Connection` 指实例。
- `Secret` 指真实秘密值，`SecretRef` 只指非秘密引用。

## 11. 术语治理

- 主架构含义以 RFC-0001 为准；本文提供跨文档规范命名。
- 新增核心术语时必须给出定义、所有者、生命周期和“不是什么”。
- 如果实现需要给现有概念换名，应先更新本文和结构图，再更新 Schema 与代码。
- 文档 Review 应搜索第 9 节列出的歧义词，并确认它们只出现在解释、非目标或替代方案中。
