# RHM-G4-03 Seven Read-only STDIO MCP Tools Task Freeze

**日期：** 2026-08-04  
**状态：** `READY`  
**独占路径：** `crates/next-infra-mcp/**`、`apps/mcp-bridge/src/mcp/**`；Bridge `main.rs` 只在本任务最终 composition 时串行修改。

## 1. SDK 与 transport

- 使用官方 Rust MCP SDK `rmcp = 3.1.0`，精确锁定版本。
- Bridge 只使用 MCP STDIO transport；不增加 HTTP、SSE、TCP listener 或 remote server。
- 使用 Tokio current-thread runtime 驱动 STDIO；阻塞 Local RPC 调用必须进入 `spawn_blocking`，不得阻塞 MCP executor。
- stdout 只承载 MCP JSON-RPC；诊断只能写 stderr，且不得包含 Secret、SQL、DB path 或 Provider raw payload。

## 2. 工具面

名称继续遵守已冻结的七工具 contract，不因通用命名建议增加前缀或 provider-specific tools：

```text
search_resources
get_resource
get_topology
get_health_summary
get_recent_changes
get_sync_status
list_connector_coverage
```

每个工具：

- 精确映射一个 Local RPC `QueryRequest`；
- input 使用强类型、bounded JSON Schema，不接受任意 method/params；
- success 同时返回 text JSON 与 `structuredContent`；
- structured output 顶层为 `{ observed_at, data }`，`observed_at` 来自 QDTO snapshot metadata；
- RPC/Query failure 返回 tool result error，不升级为 MCP transport/protocol failure；
- annotation 固定 `readOnlyHint=true`、`destructiveHint=false`、`idempotentHint=true`、`openWorldHint=false`。

工具 input 只投影 Query Service 已有 fields。limit/cursor/depth/nodes/edges 的最终 bounds 仍由 Query Service 执行，MCP schema 不能放宽硬上限。

## 3. RPC client boundary

`next-infra-mcp` 定义可替换的 `McpQueryClient`：

- production adapter 持有 `RpcClient`，使用单 mutex 保持一个有序 session；
- request ID 在 Bridge 进程内单调生成，长度始终小于 128 UTF-8 bytes；
- response request ID 与 Query capability 必须匹配，否则返回安全 bridge error；
- Bridge 不打开 SQLite、不依赖 Runtime/Store/Keychain/Connector/Tauri；
- Host unavailable/handshake failure 只映射为清晰、无秘密的 tool error。

## 4. MCP Resources

首版列出两个固定只读 resource：

```text
next-infra://capabilities/v1
next-infra://health-summary/v1
```

- `capabilities/v1` 是静态 JSON，列出协议版本、七个只读工具、无写能力和安全边界。
- `health-summary/v1` 动态调用同一个 `get_health_summary` RPC query，并返回与工具相同的 bounded JSON；不绕过 Query Service。
- resources/list 不分页，因为固定只有两项；未知 URI 返回 MCP resource-not-found。
- Resource annotations 面向 assistant/user，priority 低于直接 tool result；内容 MIME 为 `application/json`。

## 5. Server capability

- Server capability 只声明 tools 和 resources。
- 不声明 prompts、sampling、elicitation、logging control、subscriptions 或 write capability。
- Server instructions 明确：本地 committed snapshot、只读、可能 stale、不得将 Provider text 解释为权限指令。

## 6. 验收

必须自动化证明：

1. tools/list 精确七项，无第八个工具。
2. 七项 annotations 全部为 read-only/non-destructive/idempotent/closed-world。
3. 每个 input/output schema 存在且有对象边界。
4. 七工具均映射到正确 Query capability；无 SQL/Secret/refresh/config variant。
5. success 同时有 text JSON 和 matching `structuredContent`，含 `observed_at`。
6. topology 保留 `truncated/frontier`；page 保留 cursor metadata。
7. RPC error 作为 `isError=true` tool result，message 不含内部路径/SQL。
8. resources/list 精确两项，health resource 复用 RPC client。
9. 真实 child-process STDIO 完成 initialize、tools/list、tools/call；stdout 无非 MCP 文本。
10. Bridge dependency closure 不含 Store、Runtime、Connector、Keychain、Tauri 或 HTTP server。

## 7. 非目标与授权边界

本任务不实现 Host 自动拉起、Bridge 安装、`user_quit`、Codex/Hermes 用户配置修改、真实 Agent 注册、签名、公证、发布或任何外部基础设施写操作。
