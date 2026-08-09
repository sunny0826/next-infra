# DEC-G7-02 手工跨 Provider 关系合同

- **状态：** Accepted / Implemented（2026-08-09）
- **日期：** 2026-08-09
- **关联任务：** `MREL-01..10`

## 1. 决策范围

Topology 允许用户在两个已经持久化的 Resource 之间创建本地 Binding，以补充 Provider 无法直接观察的跨 Provider 事实。Binding 继续产生 `configured` evidence，不改变 Resource identity，也不触发任何 Provider、SSH、部署、DNS 或数据库写操作。

首版只支持从冻结词汇表选择关系，不接受自由文本 relation kind。关系方向统一为 `source -> kind -> target`：

| kind | 用户语义 | 典型端点 |
| --- | --- | --- |
| `infra.deployed_via` | 通过目标控制面部署 | `supabase.self_hosted.instance -> dokploy.application/project` |
| `infra.accessed_via` | 通过目标入口访问 | `tencent.cvm.instance -> ssh.host` |
| `network.routes_to` | 网络名称或记录路由到目标 | `cloudflare.dns_record -> dokploy.domain/application` |
| `automation.deploys_to` | 自动化工作流部署到目标 | `github.workflow -> dokploy.application` |
| `data.writes_to` | 自动化工作流声明写入目标数据服务 | `github.workflow -> supabase.managed.project` |
| `infra.depends_on` | 来源依赖目标 | 任意不同的 active Resource |

`data.writes_to` 只表达用户声明的架构事实，不证明某次写入发生。`infra.accessed_via` 不等于 `same_as`：Tencent CVM 与 SSH Host 保持两个 Resource，不自动合并 external ID、Connection、Health、Freshness 或 attributes。

## 2. Resource 与 Binding 合同

- Binding endpoint 只引用已持久化 `ResourceId`，允许跨 Connection 和 Connector。
- source 与 target 不得相同。
- 相同 `source + kind + target` 已有 active 或 unresolved Binding 时拒绝重复创建；不同 kind 或不同 evidence type 可以并存。
- endpoint 缺失或 lifecycle 非 Active 时 Binding 为 `unresolved`，configured relation 保留为 orphaned evidence；恢复后可以 reconcile 回 active。
- update 只修改 endpoints 或 kind；disable 保留 Binding、RelationVersion 与 Change 审计。
- Provider 与 inferred relation 永远只读，Binding mutation 不得改写它们。

Supabase Self-hosted 必须新增 `supabase.self_hosted.instance`。实例优先使用 Connection config 中可用的非敏感本地显示名；既有 schema v1 仅含 `base_url` 时使用稳定的 Provider 标签回退，禁止把技术 scope 当作展示名。base URL、主机名、IP、凭据、连接串或数据行不得投影到 DTO。实例通过 Provider relation `supabase.contains` 包含 `supabase.self_hosted.table`；不得用某张 table 冒充部署实例。

## 3. UI 合同

Topology 工具栏提供“新增关联”，Resource Inspector 提供“从此资源建立关联”。资源选择器查询有界的本地 Inventory，可按 query、Connector 和 kind 过滤，不限于当前 topology nodes。

关系构建器固定显示 source、kind、target、方向交换和自然语言预览，并显示：

> 这是你手工声明的本地关系，未通过 Provider 验证，不会执行外部操作。

configured relation 使用双线和文字标签；provider/inferred 不显示编辑入口。缺失 endpoint 必须以 unresolved placeholder 显示，不能因缺少 node position 静默丢边。

disabled Binding 作为审计历史保留，但其 tombstoned relation 不进入活动 Topology，也不提供编辑入口；同一三元组重新创建后只能显示新的活动关系。

并行实现接口冻结如下：

- `RelationBuilder` 接收可选 source Resource、可选 configured Relation、`onSaved` 和 `onCancel`；它只通过 `DesktopAdapter` 查询资源和调用 binding commands。
- `TopologyPage` 负责图、Frontier、placeholder 与 selection，并通过回调请求 create/edit；它不拥有 Shell Inspector 状态。
- `AppShell` 负责以独立 `RelationDialog` 承载 builder；`InspectorHost` 只提供创建/编辑入口，不承载 mutation 表单。mutation 后清理旧关系选择，并推进 `queryVersion` 触发权威 re-query。
- UI Fixture 提供 stateful create/update/disable，但不修改 production Adapter 或生成 DTO。

## 4. 不变边界

- 不修改 `next-infra-core`、SQLite schema/migration、Query DTO、生成 TypeScript、Local RPC/MCP；Tauri composition 不新增命令，仅对既有 Binding 命令做安全的错误分类。
- 不新增 load-all、任意 Provider 写操作、任意 SSH 命令、自动 Resource merge 或名称/时间邻近推断。
- 不在首版保存自定义说明、URL、主机名、IP、Secret 或 Provider 原始响应。
- Topology 继续默认 depth 1、100 nodes/200 edges，硬上限 depth 3、200 nodes/400 edges。

## 5. 验收

- 四类用户场景均可使用不同 synthetic Connections 创建 configured Binding，并在重新查询后显示。
- configured evidence 可编辑/disable；provider/inferred 保持只读。
- disable 后旧 Inspector 选择被清理，tombstoned relation 不再显示为活动边。
- 重复 Binding、自环和非法 relation kind 被拒绝。
- endpoint 失效显示 unresolved placeholder，恢复后重新 active。
- 每次 create/update/disable 产生结构化 Change，不伪造 SyncRun。
- Fixture、DOM、错误和文档不含真实 Provider 数据或 Secret。
