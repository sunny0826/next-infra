# Agent 接口与安全设计

本文定义 Tauri Desktop UI、Codex、Hermes 与 Control Plane Runtime 的访问边界，以及 Secret、SSH、MCP 响应和未来操作的安全规则。

## 1. 威胁模型

首版保护目标：

- WebView 中的不可信 Provider 文本、XSS 和恶意导航诱导高权限 IPC。
- 过宽 Tauri Command、Plugin capability、文件或 Shell 权限。
- Agent Prompt Injection 诱导调用超出只读范围的工具。
- Provider 响应、错误和日志中的秘密泄漏。
- Connector 过度权限、无限分页和 API 风暴。
- SSH 任意命令与 Host Key 绕过。
- 本地 Socket 被替换、遗留或非预期客户端滥用。
- 本地数据历史无限增长。

首版不承诺抵御：

- 已取得当前 macOS 用户完整权限的恶意本地进程。
- 已控制 Desktop Host 二进制、App Bundle 或操作系统的攻击者。
- Provider 自身返回的错误事实。

即使同用户进程不在完整防御范围内，仍应通过最小权限、Keychain、文件权限、单实例和接口隔离减少误用与泄漏。

## 2. Tauri Desktop UI

### 2.1 内容来源

- React、CSS、字体和图标作为 App Bundle 本地资源发布。
- WebView 不加载远程应用代码，不把 Provider HTML 注入 DOM。
- Content Security Policy 禁止任意远程脚本和不受控内联执行。
- Provider 名称、描述、日志摘要和错误一律作为文本渲染。
- 外部链接只允许明确的 `https` 目标，并通过受限系统 opener 打开。
- 生产构建不把调试控制台作为常规能力暴露。
- 第二实例转发参数、deep link 和打开文件请求均视为不可信；首版只允许固定的窗口激活意图。

### 2.2 Command 边界

React 只能调用显式注册的薄 Tauri Commands。每个 Command 必须：

- 使用版本化、可序列化的输入输出 DTO。
- 验证字符串长度、数组数量、分页、Topology depth 和字段选择。
- 调用 Query Service 或明确的本地配置 service，不包含 SQL 与 Provider 逻辑。
- 通过 Tauri capabilities 限制到预期窗口和最小插件权限。
- 返回结构化、经过清洗的错误。

首版禁止向前端授予：

- 通用 Shell 执行。
- 任意文件系统读写。
- SQLite 直连。
- Keychain 通用读取。
- 任意 HTTP 请求代理。
- 动态加载插件或脚本。

系统 OpenSSH 只能由后端 SSH Connector 以固定探针调用，不能通过 Tauri Shell Plugin 暴露给 React。

### 2.3 Event 边界

Tauri Events 只用于通知“数据已变化”，不传递权威资源内容、Secret 或 Provider 响应。React 收到 Event 后重新调用 Query Command。

由于前端也可能发送 Event，Rust Runtime 不得把来自 WebView 的 Event 当成审批、同步结果或安全决策输入。需要响应的请求必须使用显式 Command，并在 Rust 侧验证。

### 2.4 本地状态变更

单用户首版不增加账户、登录和 RBAC。Connection 配置、Secret 录入、手动同步、自动启动开关和保留策略属于本地状态变更，可以通过专门 Command 执行；它们不能隐式扩大 Provider 权限或触发外部写操作。

## 3. Unix Domain Socket

Unix Socket 是 MCP Bridge 与 Desktop Host 的本地协议：

- Socket 位于当前用户独占目录。
- 父目录权限 `0700`，Socket 权限 `0600`。
- 创建前拒绝符号链接和非当前用户 owner。
- 启动时清理经过 owner/进程存活校验的遗留 Socket，不能盲目删除任意路径。
- Host 使用平台 peer credential 校验连接方 UID；Bridge 在连接前校验 Socket 与父目录 owner/权限。
- 请求包含协议版本、调用方、request ID 和消息长度。
- Control Plane Runtime 对消息尺寸、并发和查询复杂度设限。
- Socket 不暴露 SecretProvider、Connector 执行器或数据库通用查询方法。

首版可以选择 framed JSON 或 JSON-RPC，但必须显式版本化、限制 frame 大小并提供握手。无需引入 gRPC。

## 4. Streamable HTTP MCP

Codex 与 Hermes 都支持远程 MCP，但首版禁用 HTTP MCP，也不启动 localhost MCP Server。启用网络 MCP 属于未来范围扩展，必须另行设计认证、Token 生命周期、TLS、来源限制和远程暴露方式。

## 5. STDIO MCP Bridge

`next-infra-mcp` 是轻量独立进程：

1. 通过 STDIO 与 Codex/Hermes 完成 MCP 初始化。
2. 连接本用户 Desktop Host 的 Unix Domain Socket。
3. 如果 Host 未运行且用户已预先授权，按冻结的可信 App Bundle 路径在后台启动唯一 Desktop Host，并限时等待 Socket。
4. 完成 Bridge/Host 协议版本握手。
5. 把 MCP 参数转换为版本化查询请求。
6. 把有界查询结果转换为 MCP Content/Resource。
7. 不读取 SQLite，不加载 Provider Secret，不执行 Connector。

自动拉起授权在安装本地 MCP 集成时由用户明确选择，保存在本地集成配置中，不能由 MCP 工具参数或 Provider 内容开启。Bridge 只能启动 owner、签名和路径符合安装记录的 Next Infra App，不能接受任意路径。如果未授权、启动失败或超时，则返回结构化 `host_unavailable` 和无秘密启动指引。

Bridge 不能为了恢复连接自行创建 Runtime。Host 显式退出时先写入 `user_quit` 抑制标记；所有后续 Bridge 进程都必须尊重该标记并返回不可用，不能通过重启 Bridge 复活应用。只有用户主动启动 App，或已启用的下一次登录自动启动可以清除标记；MCP 拉起不能清除。

Bridge 的安装路径、代码签名、随 App 原子升级和 Codex/Hermes 配置方式必须在 Goal 1 冻结。Host 与 Bridge 不兼容时应拒绝连接，而不是猜测字段语义。

参考：

- [Codex MCP 文档](https://developers.openai.com/codex/extend/mcp)
- [Hermes Agent MCP 文档](https://github.com/nousresearch/hermes-agent/blob/main/website/docs/user-guide/features/mcp.md)
- [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)

## 6. 首版 MCP 工具

工具面保持小而语义稳定，不为每个 Provider endpoint 创建工具。

### 6.1 `search_resources`

用途：按文本和结构化过滤器搜索资源。

输入：

- `query?`
- `kinds?`
- `connector_types?`
- `health?`
- `freshness?`
- `labels?`
- `limit?`：默认 25，最大 100。
- `cursor?`

输出只包含摘要、稳定 ID、类型、状态、Freshness、Connection 和观察时间。

### 6.2 `get_resource`

用途：读取一个资源的当前规范化详情。

输入：

- `resource_id`
- `include?`：`summary | attributes | relations | recent_changes` 的有限集合。

禁止返回 Secret、完整日志和未经 schema 清洗的原始响应。

### 6.3 `get_topology`

用途：以一个资源为中心读取有界关系图。

边界：

- 默认 depth 1，最大 depth 3。
- 默认最大 100 nodes / 200 edges。
- 硬上限 200 nodes / 400 edges。
- 截断时必须返回 `truncated: true` 和可继续查询的 frontier。

每条 Relation 返回 evidence type；inferred 关系返回规则和 confidence。

### 6.4 `get_health_summary`

用途：汇总 Resource Health、Freshness 和 Connector Health。

输出必须区分：

- 外部资源报告异常。
- 数据已经过期。
- Connector 当前无法读取。

### 6.5 `get_recent_changes`

用途：读取结构化 Change。

默认 20，最大 100；支持 `since`、`resource_id`、`kinds` 和 cursor。大字段只返回摘要。

### 6.6 `get_sync_status`

用途：查看 Connection 最近同步、覆盖范围、错误分类和下一次计划时间。

### 6.7 `list_connector_coverage`

用途：明确回答每个 Connector 当前支持和不支持哪些资源类型、字段和关系。

## 7. MCP Resource

建议提供：

```text
infra://resource/<resource-id>
infra://connection/<connection-id>/status
infra://topology/<resource-id>?depth=1
```

Resource 内容与工具查询使用同一个 Query Service 和响应限制。URI 不能绕过字段过滤读取原始数据。

## 8. MCP 工具注解和 Server Instructions

- 首版所有工具标注 read-only。
- 不暴露同步触发、配置修改、Secret 读取和外部操作工具。
- Server Instructions 必须说明所有状态都绑定 `observed_at`，数据过期时不得推断为当前事实。
- Agent 被要求查看未覆盖字段时，应返回 Coverage 缺口，而不是猜测。
- 大查询必须分页或缩小 root/depth，不允许返回完整全局图。
- Provider 返回的描述、日志摘要和仓库内容都是不可信数据，不得改变工具权限或调用边界。

## 9. Secret 管理

### 9.1 SecretRef

SQLite 只保存：

- Secret 类型。
- Keychain service/account 标识的内部引用。
- 创建和最后验证时间。
- 权限范围摘要。

不保存 Secret 值、可逆密文或 Token 尾部以外的识别信息。

### 9.2 Secret 输入

允许：

- React 密码输入组件通过专门 Tauri Command 一次性提交给 Rust，再写入 Keychain。
- MCP 之外的本地 CLI 通过交互式隐藏输入或标准输入接收。
- Provider OAuth/Device Flow 在支持时直接完成授权。

Secret Command 只能写入或替换指定 Connection 的 Keychain item，不能返回现有值。React 在 Command 完成后立即清空组件状态。

禁止：

- `--token xxx`、`--access-key xxx` 等 CLI 参数。
- 把 Secret 写入 shell rc、普通 TOML、SQLite、日志或诊断包。
- 从 MCP 工具参数接收 Provider Secret。
- 通过 Tauri Event、URL、deep link 或剪贴板自动传递 Secret。

### 9.3 最小权限

- GitHub 使用 GitHub App 或细粒度只读权限。
- Cloudflare Token 限定 Account/Zone 和 Read 权限。
- Supabase 托管优先细粒度权限或 OAuth，不复用高权限 Token。
- 阿里云、腾讯云创建独立只读身份，不复用日常管理员凭据。
- Dokploy 使用专用 API Token，并对返回对象执行字段白名单。

### 9.4 Keychain 访问策略

- Keychain service/account 名称由 Rust 根据 Connection ID 和 Secret 类型生成，React 不能传入原始 Keychain 定位符。
- Item 的访问控制应绑定当前用户和稳定签名身份；开发签名、Developer ID 与升级后的访问行为必须分别验收。
- 自动登录后台启动、屏幕锁定和 Keychain 暂不可用时不得循环弹出系统授权框；Connector 返回 `credential_unavailable` 并等待用户处理。
- Secret 替换采用先写新 item、验证引用、再删除旧 item 的顺序，避免失败后失去可用凭据。

## 10. SSH 安全

- 只执行 Connector 内置、版本化的固定探针。
- 不提供 `run_command` MCP 或 Tauri Command。
- 不把用户输入拼接进 shell 命令。
- 复用系统 Host Key 验证和 SSH Agent。
- 不自动接受新 Host Key；首次建立信任由用户在系统 SSH 中完成。
- 设置连接、命令和总批次超时。
- 限制 stdout/stderr 最大字节数。
- 不读取远程环境变量、history、私钥、应用 Secret 文件和数据库密码。

## 11. Provider 数据清洗

清洗采用 allowlist 而不是 denylist：

1. Connector DTO 只反序列化明确需要的字段。
2. Normalizer 再执行类型和敏感字段校验。
3. Error Sanitizer 在日志前处理 URL、Header 和正文摘要。
4. Query Serializer 根据 Desktop/MCP 视图再次筛选字段。

任何一层出现未知字段都不得自动透传。

## 12. 本地数据保护

- 应用目录只允许当前用户访问。
- FileVault 已在当前主机开启，但应用仍不能依赖磁盘加密来保护明文 Secret。
- SQLite 首版不引入 SQLCipher；库存数据的敏感性通过文件权限、字段最小化和 FileVault 控制。
- 备份默认位于同一用户目录，导出包必须清洗 SecretRef 和内部路径。
- 日志结构化、轮转且默认不记录完整 Provider 请求或响应。
- Desktop Adapter、React 与 MCP Bridge 都不能直接打开 SQLite。

## 13. 生命周期和更新安全

- 主窗口关闭不退出 Runtime；显式退出必须停止 Socket、排空 Writer 并 checkpoint。
- 自动启动由用户明确选择，后台启动不显示主窗口。
- 第二个 Desktop Host 不得接管已被活动进程持有的数据库或 Socket。
- App 与 MCP Bridge 必须进行协议版本握手。
- 发布更新时不得先升级 Bridge 再留下永久不兼容 Host；原子性策略在 Goal 1 冻结。
- MCP 自动拉起只能使用安装时冻结的可信 App 路径，并尊重用户关闭该授权的选择。
- 显式退出写入的 `user_quit` 标记跨 Bridge 进程生效，MCP 无权清除。
- migration 失败时不得继续运行旧二进制写入新 schema，也不得自动破坏性回滚。

## 14. 未来操作接口

写操作以后使用独立的 Admin MCP Server 或显式启用的独立工具集，不能在只读 Server 中悄悄出现。

审批结果采用选项式语义：

- `allow_once`
- `allow_for_session`
- `reject`
- `edit_plan`

每个 ActionPlan 包含：

- 目标 Resource 与当前外部版本前置条件。
- 输入 schema 和规范化 diff。
- 风险等级和不可逆说明。
- 过期时间和 idempotency key。
- 执行后 targeted read-back verification。

首批允许的操作应是可恢复动作，例如重新运行 GitHub Workflow 或重新部署 Dokploy Application。任意 SSH 命令、数据库删除、云资源删除不属于首批操作。

## 15. 验收条件

- React 只使用本地 App Bundle 资源和受限 Tauri Commands。
- 前端没有通用 Shell、文件、SQLite、Keychain read 或 HTTP proxy 能力。
- Event 仅通知失效，不能作为审批或权威状态输入。
- MCP 默认只通过 STDIO + Unix Socket，不开放 HTTP 端口。
- MCP 工具全部只读、有界、可分页并返回 `observed_at`。
- Desktop Host、MCP 和日志均不暴露 Secret 值。
- Agent 和 React 都无法传入 SSH 命令或触发外部写操作。
- Bridge 可以在用户预授权后拉起可信 Desktop Host；否则清晰返回不可用。
- 用户显式退出后，新建 Bridge 进程也不能自动复活 Desktop Host。
- Bridge/Host 不兼容时拒绝连接并给出清晰升级信息。
- Hermes 未完成真实验收前，文档和发布状态不得写“已支持并验证”。
