# DEC-CONNECT-UI-01: 非 GitHub Provider 连接录入 UI

**状态:** Proposed
**日期:** 2026-08-08
**适用范围:** SSH / Dokploy / Cloudflare / Supabase managed / Supabase self-hosted / Aliyun / Tencent 共 6 类凭据模型的建连 UI 流程
**不构成:** 通用 Secret 文本框；Keychain 迁移；多 Provider 混合表单；Provider 写操作

---

## 1. 决策

为每类凭据模型单独设计两阶段（验证→确认）录入流程。复用 GitHub 的 `github_discover_*` / `github_connect` 模式，为每个 Provider 新增 `<provider>_validate`（发现/校验输入）与 `<provider>_connect`（建连+后台首次同步）。

不使用通用 Secret 文本框。不同 Provider 的字段形状、校验方式和范围选择逻辑不可对齐到同一组件。

每类模型均复用现有的 `connector.validate` 接口作为后端校验入口。Secret 沿用 GitHub 的瞬时传递+本地受限文件模式（Keychain 迁移为后续独立决策）。

---

## 2. 凭据模型分类表

| Provider | Connector Type | 凭据字段 | 敏感度 | 存储建议 | 校验入口 | 范围选择 | 同步触发 |
|---|---|---|---|---|---|---|---|
| SSH | `ssh` | `host_alias`（字符串别名，无 Secret）| 低 | `allowed_service_ids` 存入 SQLite config，其余仅内存 | `connector.validate`（SSH Agent 或 config 检查）| 无范围（probe 对固定 alias 执行）| 建连后触发首次 probe，扩展 scheduler 需要新增 `spawn_*_sync` |
| Dokploy | `dokploy` | `url` + `token`（Bearer）| 高 | Token 文件 `0700/0600`，参照 `github_live.rs` | `connector.validate`（/v1/user）| 无范围（全量 Project/Application/Server/Domain）| 建连后后台首次同步，扩展 scheduler 同上 |
| Cloudflare | `cloudflare` | `token`（API Token）| 高 | Token 文件 `0700/0600` | `connector.validate`（/user/tokens/verify）| 无范围（全量 Account/Zone）；用户录入时提示 token 权限范围 | 同上 |
| Supabase managed | `supabase-managed` | `token`（Bearer access token）| 高 | Token 文件 `0700/0600` | `connector.validate`（/v1/projects）| 无范围（全量 Organization/Project）；用户只需提供 token | 同上 |
| Supabase self-hosted | `supabase-self-hosted` | `url` + `token`（service key）| 高 | Token 文件 `0700/0600` | `connector.validate`（连接性检查）| 无范围（Service/DB/Runtime 三类 source）| 同上 |
| Aliyun | `aliyun` | `access_key_id` + `secret_access_key`（HMAC-SHA1 签名）| 高 | 两字段存入同一 Token 文件 `0700/0600` | `connector.validate`（DescribeRegions 或类似）| 需要用户指定 `region`（云厂商必须指定同步区域）；默认空，验证时返回需要 region 的错误 | 同上 |
| Tencent | `tencent` | `secret_id` + `secret_key`（TC3-HMAC-SHA256 签名）| 高 | 两字段存入同一 Token 文件 `0700/0600` | `connector.validate`（ DescribeInstances）| 需要用户指定 `region`；默认空，验证时返回需要 region 的错误 | 同上 |

**关键约束：**
- Token 文件命名：`{provider}-secrets-v1/{connection-id}.{ext}`，目录 `0700`，文件 `0600`。
- SQLite 只存 `Connection.config`（非敏感字段如 `host_alias`、`url`、`region`）和 `Connection.secret_ref`（指向文件的路径或引用），不存明文 Secret。
- 所有 Provider 的 `connector.validate` 已在各 connector 实现中定义（详见各 `lib.rs` 的 `validate` 方法）。

---

## 3. 各模型流程设计

### 3.1 SSH（`ssh`）

**凭据性质：** 无 Secret，基于现有 SSH config alias（`AuthKind::SshAgent`）。

**UI 状态机：**

```
[输入 alias] → [验证中: 执行 SSH probe 探针] → [成功: 显示 host/已发现服务] 或 [失败: host_key_mismatch / timeout / probe_budget_exceeded]
```

**字段：**
- `display_name`: 连接名称（用户输入）
- `host_alias`: SSH config 中的 alias（用户输入，格式：`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`，见 `crates/next-infra-connector-ssh/src/config.rs` `HostAlias::parse`）
- `connect_timeout_secs`: 默认 10
- `probe_profile`: 固定 `baseline-v1`

**后端命令（新增）：**
- `ssh_validate(request: SshValidateRequest) -> SshValidateResult`：执行 `baseline-v1` probe 探针，返回已发现服务（host / filesystem / process_summary / launchd_service / systemd_service）。`singleton AtomicBool` 防并发（复 GitHub 单飞守卫模式）。
- `ssh_connect(request: SshConnectRequest) -> SshConnectResult`：创建 Connection，存入 SQLite config（含 `host_alias`、`connect_timeout_secs`、`probe_profile`、`allowed_service_ids`），注册 scheduler（间隔 300s）。

**范围选择：** 无范围选择步骤。probe 结果在验证阶段展示，用户确认后建连。

**同步触发：** `spawn_ssh_sync`（新增）复用 `spawn_github_sync` 结构，但目前 scheduler `has_live_sync_path` 仅支持 github（见 `scheduled_sync.rs:99`）。**扩展点：** 需在 `composition/mod.rs` 新增 `has_live_sync_path` 分支，并实现 `spawn_ssh_sync`。

**参考：** `crates/next-infra-connector-ssh/src/descriptor.rs`（`AuthKind::SshAgent`），`crates/next-infra-connector-ssh/src/config.rs`（`HostAlias` 格式校验），`DEC-G6-01`。

---

### 3.2 Dokploy（`dokploy`）

**凭据性质：** `AuthKind::Token`，Bearer token。

**UI 状态机：**

```
[输入 url + token] → [验证中: POST /v1/user 验证 token] → [成功: 显示可访问范围摘要] 或 [失败: invalid_token / unreachable]
```

**字段：**
- `display_name`: 连接名称
- `url`: Dokploy 实例 URL（如 `https://dokploy.example.com`）
- `token`: API token

**后端命令（新增）：**
- `dokploy_validate(request: DokployValidateRequest) -> DokployValidateResult`：调用 `/v1/user` 验证 token 可达性，返回已访问项目数/应用数摘要（不入 SQLite，不记录原始响应）。
- `dokploy_connect(request: DokployConnectRequest) -> DokployConnectResult`：创建 Connection，config 含 `url`（非敏感），secret 存文件，建连后触发首次 Full 同步。

**范围选择：** 无范围选择（Full sync 全量 Project/Application/Server/Domain）。验证通过后直接创建连接。

**同步触发：** 复用 `spawn_github_sync` 模式，新增 `spawn_dokploy_sync`；scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-dokploy/src/descriptor.rs`（`AuthKind::Token`），`crates/next-infra-connector-dokploy/src/auth.rs`（Bearer header 构造），`DEC-G8-01`。

---

### 3.3 Cloudflare（`cloudflare`）

**凭据性质：** `AuthKind::Token`，API Token。

**UI 状态机：**

```
[输入 token] → [验证中: GET /user/tokens/verify] → [成功: 显示账户摘要] 或 [失败: invalid_token / insufficient_permissions]
```

**字段：**
- `display_name`: 连接名称
- `token`: Cloudflare API Token

**后端命令（新增）：**
- `cloudflare_validate(request: CloudflareValidateRequest) -> CloudflareValidateResult`：调用 `/user/tokens/verify`，返回账户 email/id 摘要。
- `cloudflare_connect(request: CloudflareConnectRequest) -> CloudflareConnectResult`：创建 Connection，secret 存文件，触发首次 Full 同步。

**范围选择：** 无范围选择（Full sync 全量 Account/Zone/DNS/Tunnel/Worker 元数据）。验证时展示 token 权限范围摘要（账户级别/Zone 级别）。

**注意：** `descriptor` 要求 token 必须 scoped 到特定 Account 和 Zone 并只有 Read 权限。验证失败时展示 `insufficient_permissions` 而非 `invalid_token`。

**同步触发：** 新增 `spawn_cloudflare_sync`，scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-cloudflare/src/descriptor.rs`（`AuthKind::Token`，最小权限列表）。

---

### 3.4 Supabase managed（`supabase-managed`）

**凭据性质：** `AuthKind::Token`，Bearer token（Management API）。

**UI 状态机：**

```
[输入 token] → [验证中: GET /v1/projects] → [成功: 显示 Organization/Project 列表] 或 [失败: invalid_token]
```

**字段：**
- `display_name`: 连接名称
- `token`: Supabase access token

**后端命令（新增）：**
- `supabase_managed_validate(request: SupabaseManagedValidateRequest) -> SupabaseManagedValidateResult`：调用 `/v1/projects`，返回 Organization + Project 数量摘要。
- `supabase_managed_connect(request: SupabaseManagedConnectRequest) -> SupabaseManagedConnectResult`：创建 Connection，secret 存文件，触发首次 Full 同步。

**范围选择：** 无范围选择（Full sync 全量 Organization/Project）。

**注意：** 当前 `connector.validate` 仅校验 config schema 版本为空（`config: {}`），不调用 API（见 `crates/next-infra-connector-supabase-managed/src/lib.rs:50`）。**开放决策：** 需确认 `validate` 是否应实际调用 `/v1/projects` 做实时可达性验证，还是仅做 schema 校验。

**同步触发：** 新增 `spawn_supabase_managed_sync`，scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-supabase-managed/src/lib.rs`（`validate` 实现，config schema 为空）。

---

### 3.5 Supabase self-hosted（`supabase-self-hosted`）

**凭据性质：** `AuthKind::Token`，URL + service key。

**UI 状态机：**

```
[输入 url + token] → [验证中: 测试 Service API / Postgres Metadata / Fixed SSH Probe 三类 source] → [成功: 显示可用 source 类型] 或 [失败: unreachable / invalid_credentials]
```

**字段：**
- `display_name`: 连接名称
- `url`: 自托管实例 URL
- `token`: Service API key

**后端命令（新增）：**
- `supabase_self_hosted_validate(request: SupabaseSelfHostedValidateRequest) -> SupabaseSelfHostedValidateResult`：尝试读取三类 source（ServiceApi / PostgresMetadata / FixedSshProbe），返回可用 source 列表摘要。
- `supabase_self_hosted_connect(request: SupabaseSelfHostedConnectRequest) -> SupabaseSelfHostedConnectResult`：创建 Connection，config 含 `url`（非敏感），secret 存文件，触发首次 Full 同步。

**范围选择：** 无范围选择。

**注意：** self-hosted 的 config 在 `validate` 时也要求空对象（`config: {}`），与 managed 一致（见 `crates/next-infra-connector-supabase-self-hosted/src/lib.rs:155`）。

**同步触发：** 新增 `spawn_supabase_self_hosted_sync`，scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-supabase-self-hosted/src/lib.rs`（三类 source，`SourceKind`）。

---

### 3.6 Aliyun（`aliyun`）

**凭据性质：** `AuthKind::ApiKey`，AccessKey ID + SecretAccessKey（HMAC-SHA1 签名）。

**UI 状态机：**

```
[输入 access_key_id + secret_access_key + region] → [验证中: 调用 DescribeRegions] → [成功: 显示已激活模块摘要] 或 [失败: signature_mismatch / invalid_region / unreachable]
```

**字段：**
- `display_name`: 连接名称
- `access_key_id`: AccessKey ID（非敏感，可存 SQLite config）
- `secret_access_key`: SecretAccessKey（敏感，存 Token 文件）
- `region`: 区域 ID（如 `cn-hangzhou`）**（必填）**

**后端命令（新增）：**
- `aliyun_validate(request: AliyunValidateRequest) -> AliyunValidateResult`：用 `access_key_id` + `secret_access_key` 构造 HMAC-SHA1 签名调用 `DescribeRegions`，返回可用模块摘要。`region` 字段为空时返回 `invalid_region` 错误。
- `aliyun_connect(request: AliyunConnectRequest) -> AliyunConnectResult`：创建 Connection，config 含 `access_key_id`（非敏感）和 `region`；`secret_access_key` 存文件；触发首次 Full 同步。

**范围选择：** `region` 是必填字段，不是范围选择。同步覆盖该 region 下的 ECS/VPC/SLB/DNS/EIP 模块。

**注意：** `SignedRequest::new` 要求 `access_key`、`region` 均非空（`crates/next-infra-connector-aliyun/src/lib.rs:249-256`）。`secret_access_key` 通过 `SecretValue` 传递，不进入日志或 DTO。

**同步触发：** 新增 `spawn_aliyun_sync`，scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-aliyun/src/lib.rs`（`SignedRequest`，HMAC-SHA1，region 参数）。

---

### 3.7 Tencent（`tencent`）

**凭据性质：** `AuthKind::ApiKey`，SecretId + SecretKey（TC3-HMAC-SHA256 签名）。

**UI 状态机：**

```
[输入 secret_id + secret_key + region] → [验证中: 调用 DescribeInstances] → [成功: 显示已激活模块摘要] 或 [失败: signature_mismatch / invalid_region / unreachable]
```

**字段：**
- `display_name`: 连接名称
- `secret_id`: SecretId（非敏感，可存 SQLite config）
- `secret_key`: SecretKey（敏感，存 Token 文件）
- `region`: 区域 ID（如 `ap-hongkong`）**（必填）**

**后端命令（新增）：**
- `tencent_validate(request: TencentValidateRequest) -> TencentValidateResult`：用 `secret_id` + `secret_key` 构造 TC3-HMAC-SHA256 签名调用 `DescribeInstances`，返回可用模块摘要。`region` 为空时返回 `invalid_region` 错误。
- `tencent_connect(request: TencentConnectRequest) -> TencentConnectResult`：创建 Connection，config 含 `secret_id`（非敏感）和 `region`；`secret_key` 存文件；触发首次 Full 同步。

**范围选择：** `region` 是必填字段。

**注意：** TC3 签名构造见 `crates/next-infra-connector-tencent/src/lib.rs:21-67`（9 参数规范字符串）。`secret_id` 和 `secret_key` 均通过 `SecretValue` 传递。

**同步触发：** 新增 `spawn_tencent_sync`，scheduler 扩展点同上。

**参考：** `crates/next-infra-connector-tencent/src/lib.rs`（`tc3_authorization`，TC3-HMAC-SHA256）。

---

## 4. 错误与安全

**错误信封：** 统一 `ErrorEnvelope{schema_version, code, message, retryable}`。各 Provider 的 validate/connect 命令失败时，通过 `ErrorEnvelope.code` 区分：

| 场景 | code | retryable |
|---|---|---|
| 验证通过 | `ok` | — |
| Token/Secret 无效 | `authentication_failed` | false |
| 权限不足 | `permission_denied` | false |
| 凭据不可达（文件缺失/权限错误）| `credential_unavailable` | false |
| 网络/Provider 不可达 | `unreachable` | true |
| 限流 | `rate_limited` | true |
| Region 缺失（Aliyun/Tencent）| `invalid_region` | false |

**UI 展示：** 验证失败时展示具体 `message`（如"Token 权限不足：需要 Zone: DNS: Read"）；不展示原始 Provider 响应、token 值或签名细节。

**Secret 清空：** 所有密码输入框在 `finally` 块清空（与 GitHub 现有模式一致，见 `ConnectorsPage.tsx:100`）。

**禁止：** Secret 不进 SQLite、日志、错误响应、DTOs、URL 参数或命令行（引用 `DEC-G5-01-github-live-path.md` 凭据边界）。

---

## 5. 非目标与开放决策

### 5.1 Keychain 迁移
当前所有 Secret 存本地受限文件（`0700/0600`），沿用 GitHub MVP 模式。Keychain 是后续独立决策（`DEC-G1-04` 已定义技术边界），不阻塞当前 6 类 Provider 的建连 UI。

### 5.2 SSH 探针预算与即同步
SSH `probe_profile` 固定 `baseline-v1`（6 个 probe，硬预算）。验证阶段执行 probe；连接创建后触发首次 probe 同步。**开放决策：** 是否在建连后立即同步，还是等待首次 scheduler 周期（当前 scheduler 只支持 github，需要新增 `spawn_ssh_sync` 入口）。

### 5.3 云厂商 Region 默认值
Aliyun 和 Tencent 的 `region` 字段**无默认值**，验证时若 region 为空返回 `invalid_region`。是否提供 region 下拉默认值（从 DescribeRegions API 获取候选列表）作为后续增强项，不在本次范围内。

### 5.4 Supabase managed validate 实时调用
当前 `SupabaseManagedConnector::validate` 仅校验 config schema，不调用 `/v1/projects`（`crates/next-infra-connector-supabase-managed/src/lib.rs:50`）。本次 UI 设计要求 validate 实际调用 API 并展示结果。**开放决策：** 是否修改 connector 的 `validate` 行为，还是仅在 Tauri 命令层实现额外的实时验证调用。

### 5.5 组件抽象
建议提炼 GitHub 两阶段表单为可复用 `<ProviderConnectFlow>` 组件，但每个 Provider 的输入字段不同，抽象层仅提取状态机（idle→validating→success/error）和 Secret 清空逻辑。具体字段表单各自独立实现。

### 5.6 Scheduler 扩展
当前 `has_live_sync_path` 仅返回 `true` for github（`scheduled_sync.rs:99`）。所有 6 类 Provider 的定时同步需要扩展：
- 在 `composition/mod.rs` 新增 `has_live_sync_path` 的 provider 分支
- 新增各 provider 的 `spawn_{provider}_sync` 函数（参照 `spawn_github_sync`）
- 复用 GitHub 单飞守卫模式

---

## 6. 验收建议

### 6.1 组件测试（MockDesktopAdapter fixture）

每类 Provider 的建连 UI 需覆盖：

| 测试场景 | 预期结果 |
|---|---|
| 验证通过但取消建连 | Connection 不创建，Secret 文件不写入 |
| 验证失败（invalid token）| 展示 `authentication_failed` 消息，不闪退 |
| 验证失败（permission_denied）| 展示具体缺少的权限，不闪退 |
| Token 文件缺失导致同步失败 | `credential_unavailable`，scheduler skip 不 panic |
| Region 缺失（Aliyun/Tencent）| 验证返回 `invalid_region`，表单阻止提交 |
| Secret 输入框提交后清空 | React state 为空字符串 |

测试使用 `MockDesktopAdapter` 子类（如 `ssh-goal6-adapter.ts` 模式），fixture 数据不含真实 provider 信息（`notMatch(/github\.com|10\.0\.|192\.168\.|secret|password|token/i)`）。

### 6.2 真实凭据 Smoke（引用 LIVE-SMOKE-PLAN）

每类 Provider 的真实录入需按 `LIVE-SMOKE-PLAN`（`docs/HANDOFF-2026-08-07.md` §4 P1）执行：

- **SSH**: 明确授权的 alias，验证 Host Key mismatch / timeout / probe 预算
- **Dokploy**: 最小只读 token，验证 Project/Application 范围
- **Cloudflare**: scoped API token，验证 Account/Zone 范围
- **Supabase managed**: access token，验证 Organization/Project 范围
- **Supabase self-hosted**: URL + service key，验证三类 source 可达性
- **Aliyun**: AccessKey，验证指定 region 的模块覆盖
- **Tencent**: SecretId/Key，验证指定 region 的模块覆盖

所有真实响应不写入 fixture、日志、错误或 Git。

---

## 7. 实施拆解建议

### 涉及文件变更

**Desktop UI（React）：**
- `apps/desktop/src/features/connectors/ConnectorsPage.tsx`：新增 6 个 Provider 的表单 section（参照现有 GitHub 两阶段表单）
- `apps/desktop/src/platform/desktop-adapter/desktop-adapter.ts`：新增各 Provider 的 `validate` / `connect` / `preview_*_purge` / `purge_*` 接口（参照 GitHub 现有 4 个命令）
- `apps/desktop/src/generated/query/`：重新生成 DTO（`cargo test -p next-infra-query --features typescript-bindings --test export_types`）

**Tauri Host（Rust）：**
- `apps/desktop/src-tauri/src/composition/mod.rs`：新增 6×4=24 个 Tauri 命令注册（validate/connect/preview_purge/purge × 6 providers）；扩展 `has_live_sync_path` 分支
- `apps/desktop/src-tauri/src/scheduled_sync.rs`：新增 `spawn_{provider}_sync` 函数，扩展 scheduler tick 中的 provider 分支
- 新增 `apps/desktop/src-tauri/src/{provider}_live.rs`（如 `ssh_live.rs`、`dokploy_live.rs` 等），参照 `github_live.rs` 实现 Secret 文件读写

**Connector（无变更，仅引用）：**
- 各 connector 的 `validate` 方法已在代码中实现，无需修改

**Decision Doc：**
- `docs/design/decisions/DEC-CONNECT-UI-01-connection-entry-flows.md`（本文档）

### 任务拆解

| 任务 | 内容 | 依赖 |
|---|---|---|
| UI-G10-01 | SSH 建连 UI（Tauri 命令 + React 表单）| SSH connector 已完成 |
| UI-G10-02 | Dokploy 建连 UI | Dokploy connector 已完成 |
| UI-G10-03 | Cloudflare 建连 UI | Cloudflare connector 已完成 |
| UI-G10-04 | Supabase managed 建连 UI | Supabase managed connector 已完成 |
| UI-G10-05 | Supabase self-hosted 建连 UI | Supabase self-hosted connector 已完成 |
| UI-G10-06 | Aliyun 建连 UI（含 region 必填）| Aliyun connector 已完成 |
| UI-G10-07 | Tencent 建连 UI（含 region 必填）| Tencent connector 已完成 |
| UI-G10-08 | Scheduler 扩展：6 类 Provider 定时同步 | UI-G10-01~07 完成 |
| UI-G10-09 | 各 Provider 真实凭据 smoke 验收 | UI-G10-08 完成 |
