# CON-G5-01 GitHub Transport、认证与 Descriptor 任务冻结

**冻结日期：** 2026-08-05  
**状态：** `FROZEN / IMPLEMENTED / REVIEW`  
**受众：** GitHub Connector owner、后续 Repository/Actions mapper owner、Goal 5 Gate Captain  
**入口授权：** `GATE-G4` 内部实现 Review-ready；MCP Agent、Apple signing identity 与锁屏交互 smoke 按用户决定保持 deferred，不作为本任务的伪造通过项。

## 1. 目标与完成定义

本任务只建立首个真实 Provider 的只读 HTTP、认证、错误与能力描述基础。完成时，后续 `CON-G5-02/03` 可以复用同一个 GitHub client 拉取分页 JSON，但本任务本身不产生 Repository、Environment、Deployment、Workflow、Run 或 Job Observation。

完成必须同时满足：

1. 新建独立 `next-infra-connector-github` crate；内部 workspace 生产依赖只指向 `next-infra-core` 与 `next-infra-connector-api`，允许本文件冻结的第三方 HTTP/serialization 依赖，不依赖 Store、Sync、Runtime、Query、Tauri、MCP 或 Keychain 实现。
2. 固定 GitHub REST API version `2026-03-10`、`Accept: application/vnd.github+json`、`User-Agent: next-infra/0.1` 和 HTTPS `api.github.com` origin。
3. token 只从调用方临时提供的 `SecretValue` 构造敏感 Authorization header；任何配置、Debug、错误、request summary、fixture 或测试快照都不能含 token、SecretRef 标识或完整响应体。
4. transport 支持有界分页、ETag/`If-None-Match`、`304 Not Modified`、主/次 rate limit 与结构化 permission/auth/network/invalid-response 错误。
5. fake transport 可确定重放成功、分页、304、401、403 permission、403/429 rate limit、reset/retry-after 和中途分页失败，不进行真实等待。
6. Descriptor 通过公共 conformance，逐 module 公开 Goal 5 的 planned/partial coverage，不把 transport readiness 冒充资源已采集。
7. 不使用真实 GitHub token，不录制 live response；live identity 未配置只能在后续纵切报告中标记 deferred/blocked。

## 2. 冻结的官方 API 基线

实施以 2026-08-05 查阅的 GitHub 官方文档为准：

- [REST API versions](https://docs.github.com/en/rest/about-the-rest-api/api-versions)：所有请求显式发送 `X-GitHub-Api-Version: 2026-03-10`。
- [Getting started with the REST API](https://docs.github.com/en/rest/using-the-rest-api/getting-started-with-the-rest-api)：发送推荐 media type、版本与认证 headers。
- [Pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)：只跟随 `Link` header 中 `rel="next"`；列表 endpoint 使用 `per_page=100`；同时受本项目 page/item budget 限制。
- [Best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)：使用 ETag conditional request；`304` 不计入 primary rate limit，但仍按一次 transport 请求记入本地 summary。
- [Rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)：处理 `retry-after`、`x-ratelimit-remaining`、`x-ratelimit-reset`；`403` 只有在 rate-limit headers/响应语义证明时才映射为 rate limited，普通权限不足仍为 permission denied；`429` 为 rate limited。

后续 mapper 的最小 fine-grained repository permissions 已冻结为：

| 模块 | 只读权限 | 官方 endpoint |
|---|---|---|
| Repository | `Metadata: read` | List repositories for the authenticated user |
| Environment | `Actions: read` | List/Get environments |
| Deployment | `Deployments: read` | List/Get deployments |
| Workflow / Run / Job | `Actions: read` | List workflows、workflow runs、jobs |

不请求 `Contents: read`、`Administration`、Secrets、Variables 或任何 write permission。若实施发现一个冻结 endpoint 需要额外权限，立即停止并更新本文件，不能静默扩大 token scope。

## 3. Crate 与模块所有权

独占实现路径：

```text
crates/next-infra-connector-github/
  Cargo.toml
  src/
    lib.rs
    auth.rs
    client.rs
    descriptor.rs
    error.rs
    transport.rs
  tests/
    transport_contract.rs
```

Gate Captain 才能修改共享 workspace members、workspace dependency pin、dependency guard 与 `Cargo.lock`。本任务实现时允许同一串行 owner 完成这些必要 wiring，但不得顺手注册 Runtime connector 或新增 UI/MCP surface。

依赖选择冻结：

- `reqwest = 0.12.24`，关闭默认 features，只启用 `json` 与 `rustls-tls`；禁止 native TLS 与 cookie store。该版本由 2026-08-05 的 docs.rs crate metadata 固定。
- `url = 2.5.8` 用于 origin 与 Link target 校验。
- method/header/status 使用 reqwest 暴露的 typed HTTP types，不额外引入一份字符串型 HTTP 语义。
- `async-trait`、`serde`、`serde_json` 复用 workspace pin。
- 版本必须以 `=x.y.z` 写入 workspace dependency；实现前由 Cargo/官方 crate metadata 固定当前兼容 patch，并纳入 lockfile。

## 4. Transport 合同

### 4.1 请求

`GitHubRequest` 只能包含：

- `GET` 或 `HEAD`；其他 method 构造时拒绝。
- 相对 endpoint path 与 allowlisted query pairs，或由上一响应解析出的同 origin `next` URL。
- 可选 ETag；不得接受调用方提供 Authorization、Cookie、Host、User-Agent 或 API version header。
- endpoint/module 标签仅用于非秘密统计，不包含 owner、repo、URL、query value 或 token fragment。

生产 client 统一注入 headers，禁用自动 redirect。若 GitHub 列表 endpoint 返回 redirect，按 invalid response 失败；本 Connector 永远不调用 logs/artifacts download endpoint。

### 4.2 响应

`GitHubResponse` 仅向 mapper 暴露：

- status；
- allowlisted headers：ETag、Link、Retry-After、X-RateLimit-Limit/Remaining/Reset/Resource；
- 有最大字节数的 body bytes，仅在调用栈内反序列化，不落盘、不进入 Debug/error；
- 本地 elapsed/request counters。

默认预算：

| 预算 | 冻结值 |
|---|---:|
| 同步并发 | 2 |
| 单 endpoint 最大页数 | 20 |
| 每页请求条数 | 100 |
| 单响应 body | 4 MiB |
| 单次请求 deadline | 15 s |
| 整批请求数 | 200 |
| 单 endpoint 最大 item 数 | 2,000（20 页 × 100） |

mapper 可以收紧，不能放宽；需要放宽必须回到 Descriptor owner Review。

### 4.3 分页与缓存

- 只解析逗号分隔的标准 `Link` entries，并仅识别精确参数 `rel="next"`；target 必须由单个 `<...>` 包围。缺失 `next` 即结束；多个 `rel="next"`、无法解析的 target 或重复 next URL 都失败。
- `next` 必须是 `https://api.github.com/...`，不得携带 userinfo、fragment 或非标准端口；跨 origin、HTTP、畸形 Link 都是 `InvalidResponse`。
- 检测重复 next URL 与页数预算，防止循环。
- 第一页或各 module 的 ETag 由上层 cursor/cache 合同持有；本任务只传入/返回 opaque ETag，不持久化。
- `304` 返回 typed `NotModified`，不能伪造空 authoritative page。client 不读取 cache；后续 mapper 收到 `NotModified` 但没有对应缓存时必须返回 fatal `InvalidResponse`。
- 公开返回形状固定为 `Result<GitHubFetch, GitHubPaginationFailure>`。`GitHubFetch` 只能是 `NotModified { etag, request_summary }` 或 `Pages(GitHubPages { pages, etag, request_summary })`；`GitHubPaginationFailure` 必须携带 `completed_pages`、`request_summary` 和结构化 `ConnectorFailure`。`completed_pages.is_empty()` 为 fatal；非空时 mapper 才可构造 partial。

### 4.4 错误分类

| HTTP/transport | Connector 错误 | retryable |
|---|---|---|
| missing/empty/non-UTF-8 token | `CredentialUnavailable` / `AuthenticationFailed` | false |
| 401 | `AuthenticationFailed` | false |
| 403 且 `Retry-After` 存在，或 `X-RateLimit-Remaining` 精确为 `0` | `RateLimited` | true |
| 429 | `RateLimited` | true |
| 普通 403 | `PermissionDenied` | false |
| 404 | `NotFound` | false |
| 5xx | `ProviderUnavailable` | true |
| DNS/connect/timeout | `NetworkUnreachable` | true |
| 其他 reqwest send/body transport failure | `ProviderUnavailable` | true |
| bad Link/header/JSON/oversized body | `InvalidResponse` | false |

`retry_after_ms` 优先取可解析的整数秒 `Retry-After`；否则读取整数 Unix 秒 `X-RateLimit-Reset`，用注入的 `GitHubClock::now_epoch_seconds()` 做 saturating subtraction。两种结果都转换为毫秒并 clamp 到 `0..=3_600_000`；无法解析则为 `None`。错误 message 只能描述 endpoint class/status，不含 body、URL、owner/repo 或 header value。

allowlisted query keys 固定为：`per_page`、`page`、`visibility`、`affiliation`、`type`、`sort`、`direction`、`status`、`environment`、`ref`、`sha`、`branch`、`event`、`created`、`exclude_pull_requests`、`check_suite_id`、`filter`。新增 key 必须由 mapper owner 给出官方 endpoint 依据并回到 transport owner Review。

JSON 边界固定在 `GitHubPage::deserialize<T>`：transport 只负责有界 bytes，typed deserialize 失败映射 `InvalidResponse`；错误与 Debug 不包含 body。`ProviderRequestSummary.elapsed_ms` 是各 transport 请求 monotonic wall duration 的 saturating sum，status class 只记录 `1xx..5xx` 聚合键。

## 5. Descriptor 合同

`connector_type = "github"`，`connector_version = "1.0.0"`，`config_schema_version = 1`，认证类型为 `Token`，支持 `Full` 与 `Targeted`；Incremental 在明确 cursor/ETag 语义前不宣称支持。

最低权限按 module 明列：`Metadata: read`、`Actions: read`、`Deployments: read`。Coverage 必须列出 Repository、Environment、Deployment、Workflow、Run、Job 及显式 relations，但在 mapper 完成前均标记 `Partial`，reason 固定为对应 `CON-G5-02` 或 `CON-G5-03` 尚未实现。known gaps 至少包含：不读取 logs、artifacts、secrets、variables；不支持 write API；live identity 未配置不影响 fixture/contract 结论。

显式 relations 冻结为：`github.repository_environment`（Repository → Environment，`github.contains`，G5-02）、`github.repository_deployment`（Repository → Deployment，`github.contains`，G5-02）、`github.repository_workflow`（Repository → Workflow，`github.contains`，G5-03）、`github.workflow_run`（Workflow → Run，`github.executes`，G5-03）、`github.run_job`（Run → Job，`github.contains`，G5-03）。

## 6. 测试矩阵

必须有自动化证据：

1. headers 精确包含 Accept、API version、User-Agent，Authorization 被标记 sensitive 且 Debug/错误不可见。
2. 空 token、换行/header injection、非 UTF-8 token 被拒绝。
3. 同 origin 两页 pagination 成功；无 Link 结束；跨 origin、HTTP、重复 next、超过 20 页失败。
4. ETag → If-None-Match；304 返回 `NotModified` 而非空 page。
5. 401、普通 403、rate-limit 403、429、404、5xx、timeout 分类正确。
6. Retry-After 与 reset 使用 fake clock，无真实 sleep；retry delay 有上限。
7. response body 超过 4 MiB 在反序列化前失败；错误不包含 body sentinel。
8. `ProviderRequestSummary` 只含次数、耗时和 status class，不含 URL/header/body。
9. Descriptor validation 与公共 `check_descriptor` 均无 issue，Coverage 不混入 runtime health。
10. crate dependency closure 不含 Store、Sync、Runtime、Query、Tauri、MCP、Keychain/native TLS/cookie store。
11. production transport 固定 15 秒 deadline并禁用 redirect；Descriptor 的并发 guidance 固定为 2。client 不自行调度并发，后续 orchestrator 必须遵守或收紧该上限。

验证命令：

```bash
rtk cargo test -p next-infra-connector-github
rtk cargo clippy -p next-infra-connector-github --all-targets -- -D warnings
rtk cargo fmt --all --check
rtk pnpm --dir apps/desktop check:cargo-deps
```

## 7. 非目标与停止条件

- 不实现 mapper、Runtime registry、同步调度、UI、MCP serializer 或 live smoke。
- 不创建或修改 Keychain item，不读取环境变量中的 token，不调用 `gh auth token`。
- 不下载日志、artifact、archive，不访问 Secrets/Variables endpoints。
- 不接受任意 GitHub Enterprise base URL；GHES 需要单独决策其 origin、TLS 与 API version 支持。
- 需要修改共享 Connector API、Core domain、SecretProvider 或 Sync semantics 时立即停止并回派，不在 GitHub crate 内建立平行语义。

## 8. Reader Review 问题

实现前 reviewer 必须能仅凭本文回答：

1. 为什么 `304` 不能被 mapper 当作空 authoritative page？
2. 403 在什么证据下才是 rate limited，而不是 permission denied？
3. token 在哪个边界进入请求，哪些持久化/日志位置永远不可达？
4. 中途分页失败如何让 mapper 形成 partial，而第一页失败为何是 fatal？
5. 哪些 module 权限是最低只读权限，为什么不请求 Contents 或 Administration？
6. 什么条件会触发回到 Descriptor owner，而不是在 mapper 中放宽预算或权限？

若任一问题答案存在两种合理解释，本冻结文档必须先修订，不能开始实现。

## 9. 实施结果

2026-08-05 已按本冻结完成 `next-infra-connector-github` transport/auth/client/error/descriptor shell；详细证据见 [`CON-G5-01-2026-08-05.md`](./CON-G5-01-2026-08-05.md)。Repository/Actions mapper、Runtime 注册、UI/MCP 纵切和 live token 验收仍不属于本任务。
