# Next Infra Interface System

**状态：** Accepted prototype baseline（工程实现前）  
**适用范围：** 未来 Desktop UI 的信息架构、视觉 token、响应式布局、重复组件、状态表达、键盘交互与验收  
**不包含：** React/TypeScript、Tauri、Rust、组件库、状态库或构建配置的实现选择

## 1. 权威性与适用规则

本文固化已经由独立 HTML 原型和真实浏览器 viewport 验证的 UI 模式，供未来工程实现直接复用。它不授权开始工程开发，也不把原型 Fixture 变成产品数据。

发生冲突时按以下顺序处理：

1. 领域定义、枚举值和架构边界以 [`docs/design/glossary.md`](../docs/design/glossary.md) 与主 RFC 为准。
2. UI token、布局测量值、重复组件和状态表达以本文为准。
3. 交互目的、页面职责和查询边界以 [`docs/design/visualization-and-interaction.md`](../docs/design/visualization-and-interaction.md) 为准。
4. [`prototype/index.html`](../prototype/index.html) 是可运行的行为与视觉参考，不是生产实现。
5. 概念图只提供构图参考，不是文案、数据、Provider 或像素值的权威来源。

只把满足以下任一条件的模式写入本文：在两个以上页面重复出现；是稳定的应用框架；具有已经浏览器验证的具体测量值。资源名、时间、数量、ID、SyncRun 编号和临时状态均为 Fixture，不得固化。

## 2. 设计意图

### 2.1 具体使用者

使用者是当前 Mac 的唯一基础设施维护者。他会在日常巡检、部署失败、域名异常、远程主机失联，或 Codex/Hermes 查询后打开应用。他需要在切换 Provider 控制台前先判断本地 Snapshot 是否可信，并能追溯事实来源。

### 2.2 核心任务

界面始终服务四个动词：

1. **发现**：找出真实异常、过期事实、连续同步失败和 Coverage 缺口。
2. **定位**：从一个 Resource 沿有证据的 Relation 查看上下游。
3. **核实**：分开判断 Resource Health、Freshness 与 Connector Health。
4. **回放**：按 SyncRun、Binding 或 Inference 查看结构化 Change 和版本来源。

### 2.3 体验意图

体验应是“冷静、紧凑、证据优先的基础设施图谱”：像一张持续更新、可核实的工程地图，而不是 NOC 大屏、Provider 控制台拼盘或彩色 KPI 卡片墙。

## 3. 产品领域与视觉方向

### 3.1 Domain

- **Inventory**：当前已知 Resource 的清单。
- **Topology**：依赖、部署、路由与承载关系。
- **Observation**：Connector 在特定时间从 Provider 读取的候选事实。
- **Freshness**：已保存事实是否仍足以代表当前状态。
- **Evidence**：Relation 成立的 Provider、Binding 或 Inference 来源。
- **Coverage**：Connector 能观察什么，以及一次 SyncRun 实际观察了什么。
- **Change**：相邻 ResourceVersion 或 RelationVersion 的结构化差异。
- **Heartbeat**：同步成功、退避和下一次计划。

### 3.2 Color world

- 机架石墨：应用画布和密集数据区域。
- 阳极氧化铝灰：边界、层级和非活动状态。
- 遥测青：选择、焦点、数据流向和可交互强调。
- 心跳绿：被文字限定的成功、健康或新鲜状态。
- 检查琥珀：stale、expired、partial、running 和需要核实的状态。
- 故障朱红：明确失败、不可达和 unhealthy。
- 云雾灰蓝：unknown、disabled 和未覆盖。

Provider 通过名称和单色线性 glyph 识别。颜色只表达状态与交互，不按 Provider 给页面、节点或大面积区域染色。

### 3.3 Signature：Evidence Spine

**Evidence Spine（证据脉络）** 是产品的识别性结构：在同一条可阅读路径中串联 Provider/Connection、Observation/SyncRun、当前 Projection、Relation 或 Change。具体规则见第 8 节。

### 3.4 Rejected defaults

| 拒绝的默认方案 | 替代模式 |
| --- | --- |
| 顶部四张大数字 KPI 卡 | Attention Queue、Observation Strip 和有来源的状态带 |
| 一次加载全局节点图 | 以选中 Resource 为中心，depth 1 起步，按 Frontier 展开并设置硬上限 |
| Provider 彩虹配色 | 单色 glyph/标签识别 Provider，语义色只表达状态与交互 |
| Card grid 与嵌套卡片 | 密集表格、连续分区画布和 Evidence Spine |
| 日志终端式 Timeline | 按 origin 分组的结构化 Change |

## 4. 视觉 token

### 4.1 颜色

以下名称和值与 `prototype/index.html` 的 `:root` 完全一致；实现不得以近似色替换。

| Token | 值 | 用途 |
| --- | --- | --- |
| `--rack-canvas` | `#0d1115` | 应用基础画布 |
| `--rack-surface` | `#11171c` | 表格、摘要与控件 surface |
| `--rack-raised` | `#151c22` | 轻微提高的 surface |
| `--rack-hover` | `#182127` | hover surface |
| `--rack-selected` | `#15242a` | selected surface |
| `--aluminum-line` | `#29333b` | 标准边界 |
| `--aluminum-strong` | `#3a4650` | 强边界与抽屉分隔 |
| `--label-primary` | `#e2e8ec` | 主文本 |
| `--label-secondary` | `#9ca8b1` | 辅助文本 |
| `--label-muted` | `#6f7b84` | 元数据、占位与弱文本 |
| `--telemetry-cyan` | `#4fc7dc` | 焦点、选择和主交互强调 |
| `--telemetry-cyan-soft` | `#1b6572` | cyan 的低对比边界 |
| `--heartbeat-green` | `#69c58b` | 带文字限定的成功语义 |
| `--inspection-amber` | `#e2b55e` | 需要检查、过期或部分结果 |
| `--fault-vermilion` | `#ef715f` | 明确故障语义 |
| `--cloud-unknown` | `#75899a` | unknown、disabled、未覆盖 |

应用框架中还有经过 viewport 验证的结构 surface：Navigation `#0f1418`、Context bar/Inspector `#10161a`、Runtime bar `#0c1114`、Topology canvas `#0c1115`。未来实现应把这些值提升为语义变量，不得在组件中继续散落字面量。

### 4.2 字体与数字

| Token/层级 | 精确值 |
| --- | --- |
| `--font-ui` | `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` |
| `--font-mono` | `ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace` |
| 应用基础文字 | `13px`, UI 字体 |
| 页面标题 | `18px / 620 / -0.02em` |
| Inspector 标题 | `15px / 620 / 1.35` |
| Section 标题 | `11px / 650 / 0.055em`, uppercase |
| Eyebrow | `9px`, mono, `0.08em`, uppercase |
| 表头 | `9px / 500 / 0.035em`, mono, uppercase |
| 表格正文 | `11px` |
| Badge 与元数据 | `9px`, mono |

ID、kind、时间、版本、计数、Cursor 和字段路径使用 `--font-mono`；数字使用 `font-variant-numeric: tabular-nums`。字体层级必须同时依靠字号、字重、字距和字体类别，不得只靠颜色区分。

### 4.3 间距

唯一基础单位为 `4px`：

| Token | 值 |
| --- | --- |
| `--space-1` | `4px` |
| `--space-2` | `8px` |
| `--space-3` | `12px` |
| `--space-4` | `16px` |
| `--space-5` | `20px` |
| `--space-6` | `24px` |

未来组件新增间距必须使用该刻度。原型中为视觉校准存在的结构测量值（例如 Inspector `14px` 内边距、桌面 Page `18px 20px 28px`）只属于对应框架模式，不得扩展成新的全局 spacing token。

### 4.4 边界、深度与圆角

- Depth 固定为 **borders-only**；surface 只做同色系的小幅明度变化。
- 标准边界：`1px solid var(--aluminum-line)`。
- 强调边界：`1px solid var(--aluminum-strong)`。
- 选中行左侧 rail：`2px solid var(--telemetry-cyan)`。
- Focus ring：`2px solid var(--telemetry-cyan)`，`outline-offset: -2px`。
- `--radius-control`：`3px`。
- `--radius-panel`：`4px`；紧凑 Badge 可使用已验证的 `2px`。
- 不使用 drop shadow、glass、glow、渐变或大圆角制造层级。

### 4.5 控件与密度测量

- Navigation item `35px`；搜索框 `30px`；普通按钮/select `29px`；Filter button `26px`。
- 表头/数据行 `30px/40px`；Switch `34×19px`、knob `11px`、位移 `15px`；SVG 图标 `16×16px`、`stroke-width: 1.6`。

### 4.6 Motion

- 唯一冻结的快速微交互为 `120ms ease`，用于 switch 的 `transform` 与 `background`。
- 页面选择、Inspector 更新和本地查询刷新应快速、无弹簧、无弹跳；不得通过持续动画暗示实时性。
- `prefers-reduced-motion: reduce` 时使用 `scroll-behavior: auto`，并把 transition duration 压到 `0.01ms`。
- 原型中的 toast 展示时长和模拟 SyncRun 时长不是设计 token，不得固化。

## 5. 应用框架与响应式规则

### 5.1 桌面三栏框架

默认框架使用 CSS Grid：

```text
columns: 188px minmax(0, 1fr) 316px
rows:    52px minmax(0, 1fr) 29px

Navigation | Context bar across canvas + Inspector
Navigation | Primary canvas | Inspector
Navigation | Runtime bar across canvas + Inspector
```

- 最小应用宽度为 `320px`。
- Navigation 与画布同属石墨色世界，只用安静边界分隔。
- Context bar 固定 `52px`，包含当前 scope、本地全局搜索、observed/sync 上下文。
- 中央画布只承载一个主任务；桌面 Page 默认 `min-width: 700px`，自身滚动。
- Inspector 固定 `316px`；选中 Resource、Relation、Change 或 Connection 时打开，关闭后中央画布占用释放空间。
- Runtime bar 固定 `29px`，持续显示 Control Plane Runtime、Connection 读取/最近同步摘要和本地数据预算；它不替代 Resource Health 或 Freshness。

### 5.2 `max-width: 1180px`

- 框架变为 `188px minmax(0, 1fr)` 两栏。
- Inspector 变为右侧 fixed drawer：`top: 52px; right: 0; bottom: 29px; width: 316px`。
- Context bar 使用 `minmax(140px, .75fr) minmax(260px, 1.25fr)`、`10px` gap。
- Context status 隐藏，但页面与 Inspector 内的 observed_at/Freshness 不能因此消失。

### 5.3 `max-width: 820px`

- Navigation 收缩为 `56px` 图标栏；隐藏品牌文字、导航文字和 sidebar foot。
- Navigation item 居中，必须保留可访问名称和当前项状态。
- Context bar 使用 `minmax(140px, 1fr) minmax(220px, 1.3fr)`、`9px` gap、水平 `10px` padding。
- Runtime bar 隐藏次要末尾 cluster，保留 Runtime 核心状态。

### 5.4 `max-width: 560px`

- Context bar 单列，隐藏重复的 scope 文本；搜索仍可用。
- Page 取消 `700px` 最小宽度，padding 为 `14px 14px 24px`；Page header 改为纵向。
- Inspector 从 `left: 56px` 到右边界，占据导航之外的可用宽度，并保持显式关闭按钮。
- 表格使用自身横向滚动，内容最小宽度 `660px`；不得让页面整体横向溢出。
- Observation Strip 使用 `repeat(5, 148px)` 的内部横向滚动。
- Critical Path 纵向堆叠并隐藏装饰连接箭头。
- Topology canvas 最小宽度 `660px` 并在容器内部滚动。
- Timeline group 单列；日期改为底边界；未来实现必须让事件内容以 `620px` 为最小内部宽度并在所属容器内滚动。
- Settings row 单列，控制项左对齐。
- Runtime bar 只保留核心状态文案。

这些断点来自 `1600×1000`、`900×800` 和 `390×844` 的浏览器验收。viewport 是验证证据，不是新的断点；它只证明响应式框架与代表性核实路径，不表示六个页面的全部契约均已逐页验收，页面级契约仍须按第 14 节逐页回归。

## 6. 全局重复组件模式

- **Navigation/Context bar：** 顺序固定为 Overview、Inventory、Topology、Timeline、Connectors、Settings；结构不随 Provider 改变。active 同时使用 selected surface、cyan 文本和边界。搜索只查询有界的本地 Resource Snapshot，选择结果进入 Inventory 并打开 Inspector；不得执行 SSH、Manual Sync、Local Configuration 或外部写操作。
- **Page/Section header：** Page header 由 mono Eyebrow、标题、任务描述和可选动作组成；Section 右侧只放范围、计数、来源或明确动作，不以大数字卡片主导页面。
- **Data frame：** 使用固定表头、稳定列/排序和有界分页。hover 为 `--rack-hover`；selected 为 `--rack-selected` 加 2px cyan 左 rail。行可聚焦，`Enter/Space` 等同点击。长 ID/URN/原始属性用 mono 折叠；横向滚动只发生在 Data frame 内。
- **Semantic badge：** 必须包含枚举文字，可附 `5px` 状态点；状态点和颜色不能单独传义。unknown、disabled、缺失覆盖使用 `--cloud-unknown`。
- **Inspector：** 是上下文检查区，不是权威数据源。顺序固定为对象类型、名称/关系、规范身份、摘要、Current facts、Evidence path；头部显示 Evidence Spine 和关闭按钮。Overview/Inventory/Topology/Timeline/Connectors 选中对象后打开，Settings 默认关闭。
- **Runtime/Notice/Toast：** Runtime bar 只显示本地 Runtime、Connection 读取和存储摘要；Notice 承载持久但未阻断的状态；Toast 只确认刚完成的本地交互，错误和待处理状态必须留在页面或 Inspector。

## 7. 六个主页面的组件契约

| 页面 | 重复组件与固定契约 |
| --- | --- |
| Overview | 按 Attention Queue → Observation Strip → Critical Paths → Recent Changes 阅读。前者含 unhealthy、expired、连续同步失败、partial Coverage；Critical Path 只来自用户固定；选择项打开 Evidence Spine；资源总数仅作次要上下文。 |
| Inventory | 组合 Filter + Result summary + 高密度表格；稳定列含 name、kind、scope、Resource Health、Freshness、Connection、observed_at、Lifecycle。选择行打开 Inspector；默认 25/50、单次不超过 100，分页或虚拟化；empty 显示 Filter。 |
| Topology | focus-centered，默认 depth 1、`100 nodes/200 edges`，硬上限 `200/400`；节点 `136×64px`，边 hit width `14px`。Toolbar 显示 focus/depth/实际数/observed_at/truncated；节点/边分别打开 Resource/Relation Inspector；Frontier 明示继续路径，禁止加载全部；图例含线型和文字。 |
| Timeline | 按日期和 `SyncRun/Binding/Inference` 分组；事件含本地时间、origin、字段路径、before/after 摘要和 Resource。选择项打开 Evidence Spine；大字段折叠，未变化轮询不产生日志；不是日志终端。 |
| Connectors | Connection 表含 Connector Health、最近成功/尝试、下一次计划/退避、Connector Coverage；矩阵使用 supported/partial/unsupported 并解释缺口。Manual Sync 返回只读 SyncRun，不能与本地刷新合并；错误分类展示；Secret 只可替换。 |
| Settings | 连续 Settings row：左侧说明、右侧单一控制。Start at login 与 MCP auto-launch 独立；`user_quit` 可见且 Agent 无权清除；只含 Retention、Data budget、redacted diagnostics 和非秘密 Local Configuration，不改 Provider Resource。 |

## 8. Evidence Spine 规范

### 8.1 固定结构

一般路径为：

```text
Provider / Connection
  -> Observation / SyncRun + observed_at
  -> Resource or Relation current Projection
  -> selected Relation / Change / Attention fact
```

- `provider evidence` 必须引用 Provider、Connection、SyncRun、字段路径和 observed_at。
- `configured evidence` 从 Binding 开始，必须显示 Binding 与创建时间，不伪造 SyncRun。
- `inferred evidence` 从规则开始，必须显示规则版本、输入 ResourceVersion/RelationVersion 与 confidence。
- Spine 的最后一步使用 cyan active marker；其他步骤仍可阅读，不得只显示最后结论。
- 相同端点有多条 Evidence 时可以在画布合并，但 Inspector 必须列出全部来源。

### 8.2 必须出现的位置

Evidence Spine 至少复用于：Overview 注意项、Inventory/Resource Detail、Topology Relation、Timeline Change、Connectors Coverage/SyncRun。它在不同页面可以改变内容长度，但不可改变 provenance 含义。

## 9. 独立状态语义

以下维度可以同时出现，禁止折叠成一个“综合状态”：

| 维度 | 规范值 | 回答的问题 |
| --- | --- | --- |
| Resource Health | `healthy | degraded | unhealthy | unknown` | Provider 最后一次 Observation 报告了什么 |
| Freshness | `fresh | stale | expired` | 已保存事实现在是否仍可信 |
| Connector Health | `healthy | degraded | auth_failed | rate_limited | unreachable | disabled` | Connection 当前还能否读取 Provider |
| Sync Coverage | `authoritative_full | incremental | partial | targeted` | 本次 ObservationBatch 实际看到了多少 |
| Connector Coverage | `supported | partial | unsupported` | Connector 实现理论上覆盖什么 |
| Lifecycle | `active | tombstoned | orphaned` | 当前 Projection 是否仍被视为存在 |

Binding 端点不可用时使用 `unresolved`，不是 Resource Health。每个状态组合必须同时显示字段名、枚举值和相关 observed_at/last success；例如最后一次 Resource Health 为 healthy、Freshness 为 expired、Connector Health 为 auth_failed 是合法且必须保留的三行事实。

颜色映射只提供视觉辅助：成功/healthy/fresh/supported/active 使用 green；stale/expired/partial/degraded/running/tombstoned/rate_limited 使用 amber；unhealthy/failed/auth_failed/unreachable 使用 vermilion；unknown/disabled/unsupported/orphaned 使用 blue-gray。文字和维度标签始终是主区分。

## 10. Relation Evidence 线型

| `evidence_type` | 冻结线型 | 必须随线显示 |
| --- | --- | --- |
| `provider` | `#64737d`、`1.3px` 实线、方向箭头 | `provider` 标签；Inspector 中显示 Connection、SyncRun、字段路径、observed_at |
| `configured` | 两条平行 `1px` 实线，中心总间距 `4px`，仅主线带方向箭头 | `configured` 标签；Inspector 中显示 Binding、创建时间 |
| `inferred` | `#64737d`、`1.3px`、`stroke-dasharray: 6 5`、方向箭头 | `inferred` 标签；Inspector 中显示规则版本、输入版本、confidence |

选中任意 Evidence 时统一使用 `--telemetry-cyan` 和 `2px` stroke，但仍保留其线型和文字标签。线型承担主要区分，不得只靠颜色。

## 11. 数据与系统状态完整性

每个数据视图都必须有下列独立状态：

| 状态 | 必须表达的行为 |
| --- | --- |
| `initial_setup` | 尚无 Connection；说明下一步 Local Configuration，不显示 0 resources 假象 |
| `loading` | 保留页面框架、表头或图边界；使用有限 skeleton，不使用无限全屏 spinner |
| `empty` | 查询成功但无匹配结果；显示当前 scope、Filter 和清除路径 |
| `stale` | 保留最后事实，显示 observed_at、stale 文字与检查琥珀 |
| `expired` | 保留最后事实并明确“不足以代表当前状态”；不得覆盖最后 Resource Health |
| `partial` | 显示已有结果、缺失范围和原因；不得伪装成 empty 或 succeeded |
| `error` | 区分本地查询、Connector、Provider 与 Host 错误；提供稳定错误分类和处理方向 |
| `truncated` | 显示当前/硬上限、Pagination Cursor 或 Frontier，以及继续入口 |
| `credential_unavailable` | 说明需要用户处理 Keychain/授权；不循环弹窗，不提示或显示 Secret 值 |
| `host_unavailable` | 说明 Desktop Host 未运行、未授权拉起、启动失败、超时或受 `user_quit` 抑制；不得返回空结果 |

`tombstoned`、`orphaned` 与 `unresolved` 必须保留实体/关系占位、最后权威观察和原因，不能从 Inventory、Topology 或历史中静默消失。

## 12. 键盘、焦点与可访问性

- 所有 button、input、select、行选择和 Topology node/edge 都有可见 Focus ring。
- `Command/Ctrl + K` 聚焦全局搜索；`Escape` 清空搜索并关闭结果列表。
- 可选择表格行和 Topology 元素在 `Enter`/`Space` 上执行与点击相同的动作。
- 未来工程实现必须补足 Topology 相邻节点的方向键导航；这是设计文档要求，HTML 原型未完成的验收项。
- Navigation 图标折叠后仍必须有可访问名称、active 状态和 Tooltip 以外的机器可读标签。
- 所有状态同时显示文字；红绿辨识、Tooltip 或颜色都不能成为唯一信息来源。
- 外部链接明确提示离开应用，并交由系统浏览器打开。
- 文本缩放后关键状态、动作和 observed_at 不得被截断。
- 遵守第 4.6 节 reduced-motion 规则，不使用粒子、持续漂浮、闪烁或弹跳。

## 13. 明确禁止项

- 大 KPI 卡片墙、通用 admin dashboard 卡片模板、嵌套 Card grid。
- 全局 Topology hairball、“加载所有资源”或无硬上限图查询。
- Provider 品牌色铺满页面、节点或表格。
- 用一个绿色圆点同时表达 Resource Health、Freshness、Connector Health、Coverage 和 Lifecycle。
- 渐变、glass、glow、大阴影、混合深度策略和大圆角。
- 把错误、partial、host unavailable 或 credential unavailable 表达成空白页面或 `0 resources`。
- 把 Timeline 做成日志终端，或把未经清洗的 Provider JSON 作为默认 Resource Detail。
- 从全局搜索触发命令、SSH、Manual Sync、Local Configuration 或外部写操作。
- 显示、复制、记录或通过 Inspector 暴露 Secret；UI 只能处理 SecretRef 的非秘密状态与替换动作。
- 用动画、倒计时或“live”文案暗示首版轮询是实时系统。
- 将原型 Fixture 文案、资源名、时间、数量、ID 或截图中的偶然内容写入产品常量。

## 14. 未来工程实现验收清单

- [ ] 颜色、字体、spacing、radius、边界、motion 均来自第 4 节；框架与三个断点精确匹配第 5 节。
- [ ] 窄屏无页面级横向溢出；表格、Topology、Timeline 只在自身容器滚动；无阴影、渐变、Provider 彩虹、大圆角或 KPI 卡片墙。
- [ ] 任一 Resource 同时保留 Resource Health、Freshness、Connector Health、Coverage、Lifecycle、observed_at；三类 Evidence 使用冻结线型、标签和 provenance。
- [ ] 五个数据页都能进入 Evidence Spine；configured 不伪造 SyncRun，inferred 显示规则、输入版本、confidence。
- [ ] Topology 上限、Frontier、truncated 和继续路径可验证；所有状态各有独立 Fixture。
- [ ] Manual Sync/本地刷新、Start at login/MCP auto-launch/`user_quit` 分别保持独立语义。
- [ ] 搜索、导航、行/图选择、Inspector 关闭可由键盘完成；reduced-motion 无持续动画。
- [ ] 窗口恢复后保留导航并重新查询权威 Snapshot；Secret 不进入 DOM、日志、toast、诊断或错误文案。
- [ ] 三种已验收 viewport 完成交互/截图回归后，再做真实 Tauri Desktop 的窗口、WebView、键盘与 Runtime bar 验收。
- [ ] 验证 Overview → Topology → Resource/Relation Inspector → Timeline 的完整核实路径。

## 15. 参考来源

- 可运行原型：[`prototype/index.html`](../prototype/index.html)
- 原型说明：[`prototype/README.md`](../prototype/README.md)
- Overview 概念构图：[`prototype/reference/overview-concept.png`](../prototype/reference/overview-concept.png)
- Topology 概念构图：[`prototype/reference/topology-concept.png`](../prototype/reference/topology-concept.png)
- 浏览器验证产物：[`output/playwright/`](../output/playwright/)
- 页面与状态设计：[`docs/design/visualization-and-interaction.md`](../docs/design/visualization-and-interaction.md)
- 规范术语：[`docs/design/glossary.md`](../docs/design/glossary.md)

概念图中的具体文案、资源、Provider、日期和用户身份均不属于本规范；未来实现只复用已经验证的结构、token、状态语义和交互路径。
