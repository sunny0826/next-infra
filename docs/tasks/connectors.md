# Connector 与 Provider 任务包

本文件覆盖共享只读 Connector 契约、Normalizer、Fixture、GitHub、SSH、Dokploy、Cloudflare、Supabase、阿里云和腾讯云。通用调度规则见[总调度手册](./README.md)。`CON-G2-01` 已进入复核，当前可并行派发 `CON-G2-02/03/04`；真实 pipeline 汇合继续等待 Store 与 Sync 分支。

## 1. 所有 Connector 的硬边界

每个 `CON-*` 任务都继承以下约束：

- Connector 只收集 Observation；不写 SQLite、不生成 Tombstone、不合并跨 Provider Resource。
- 不修改 Query、MCP、Tauri、UI、Store、Keychain、根 manifest、lockfile 或全局 registry。
- 不实现 Provider 写 API、任意 SSH 命令或 `ActionConnector`。
- 原始 Provider response 默认不落盘；DTO 使用 allowlist，未知字段丢弃。
- Fixture 只用 `fixture-*`、`example.test` 等合成值，不包含真实账户、仓库、hostname、IP 或响应。
- 稳定 `external_id`、`evidence_key` 和 Fingerprint；输入顺序变化不改变规范化输出。
- 收集到部分有效观察后发生 permission/分页/429 错误时返回 `partial` batch；无有效观察才返回结构化 fatal error。
- partial/incremental/targeted/failed 不增加缺失计数；Tombstone 只由 Sync/Writer 处理。
- 日志、错误、`provider_request_summary`、ObservationBatch 不含凭据、response body 或秘密 URL 参数。
- 429/backoff tests 使用 fake clock，不真实等待。
- 执行真实 Provider 任务时必须重新查阅并固定当时的官方 API、SDK、权限和限流文档；本拆解不把 2026-08-02 的接口假设当作永久事实。

通用验证：

```bash
rtk cargo test -p <connector-crate>
rtk cargo test -p next-infra-connector-contract-tests -- <connector-type>
rtk cargo clippy -p <connector-crate> --all-targets -- -D warnings
rtk cargo fmt --all --check
```

## 2. Goal 2：共享 Connector 基础

### `CON-G2-01` — Connector API 契约冻结

- **状态：** `REVIEW`。
- **目标：** 实现并冻结 `ConnectorDescriptor`、`ReadConnector`、`ValidationReport`、`SyncRequest`、`ObservationBatch`、Sync Coverage 和结构化错误。
- **依赖：** `RHM-G2-01` Domain Contract。
- **独占路径：** `crates/next-infra-connector-api/**`。
- **范围：** auth kind、sync mode、schema version、resource/relation capability、rate-limit guidance、redaction report 和 request summary。
- **非目标：** 无 Normalizer、数据库、调度或真实 Provider。
- **输入/输出：** Core types → 版本化 Rust API、serde/schema tests、descriptor invariants。
- **验收：** partial/fatal 分界唯一；SecretRef 与 Secret value 不可混用；Sync Coverage 使用限定类型且不等于 Connector Coverage。
- **验证：** connector-api tests/clippy。
- **实现证据（2026-08-03）：** 已实现 object-safe async ReadConnector、Descriptor/Auth/RateLimit/Capability、非秘密 Validation/SyncRequest、Resource/Relation Observation、Redaction/Request Summary，以及结构上分离 complete/partial/fatal 的 SyncOutcome。6 项契约测试覆盖请求无 Secret 字段、Coverage/mode、partial/fatal、Descriptor 重复、状态一致性与 trait object，专属 Clippy 通过。
- **风险/停止：** 这是 Provider 扇出契约；冻结前不得开始真实 Provider，后续破坏性变化必须显式升级 schema/version。

### `CON-G2-02` — Normalizer

- **状态：** `REVIEW`。
- **目标：** 将候选 Resource/Relation 规范化为 `ValidatedBatch`。
- **依赖：** `CON-G2-01`。
- **独占路径：** `crates/next-infra-normalizer/**`。
- **范围：** allowlist、schema validation、stable sort、Fingerprint、relation endpoint/evidence validation 和二次 secret scan。
- **非目标：** 无 Tombstone、跨 Provider inference、Writer 或 Provider 特例。
- **输入/输出：** ObservationBatch + attribute schema → ValidatedBatch 或清洗后的结构化错误。
- **验收：** 输入顺序不影响 Fingerprint；未知字段消失；无效端点被拒；相同关系不产生新 evidence key。
- **验证：** property、golden normalization、secret sentinel tests。
- **实现证据（2026-08-03）：** Normalizer 已实现 exact dotted-path allowlist、递归 secret scan、稳定排序、SHA-256 semantic Fingerprint、重复身份冲突检测、relation endpoint/evidence schema 校验与完整 Redaction/Request Summary 传递；输入顺序不改变输出，未知嵌套字段消失。5 项聚焦测试、专属 Clippy 和全 workspace 31 项测试通过。
- **风险/停止：** 不把缺失计数或 Provider workaround 放进 Normalizer。

### `CON-G2-03` — Fixture Connector

- **状态：** `REVIEW`。
- **目标：** 提供完全离线、确定、可重放的 Connector，供 Goal 2/3 同步和 UI 测试。
- **依赖：** `CON-G2-01`。
- **独占路径：** `crates/next-infra-connector-fixture/**`、`fixtures/connectors/fixture/**`。
- **范围：** full/incremental/targeted/partial、分页中断、429、permission、credential unavailable、变化/缺失/恢复场景。
- **非目标：** 不模仿任何真实 Provider API。
- **输出：** schema-valid ObservationBatch 序列和确定性 snapshots。
- **验收：** 覆盖相同批次、变化批次、连续两次权威缺失、partial 后恢复和 evidence 稳定性。
- **验证：** Fixture tests、replay snapshots。
- **实现证据（2026-08-03）：** FixtureConnector 已使用仓库内 `replay-v1.json` 提供确定性的 full、incremental、targeted、partial→recovery、连续 authoritative missing 与 fatal 场景；同一 mode/cursor 请求可重放为相同结果，fixture 不接受 Secret，且无真实 URL、账户、主机或 IP。3 项 Fixture 测试、专属 Clippy 和全 workspace 34 项测试通过。
- **风险/停止：** Fixture 不成为生产 Connector 的共享逻辑入口。

### `CON-G2-04` — Contract Tests 与 Coverage Catalog

- **状态：** `READY`。
- **目标：** 建立所有 Connector 复用的 conformance suite，并将 Descriptor 投影为 Connector Coverage Snapshot。
- **依赖：** `CON-G2-01`；最终验收依赖 `CON-G2-02/03`。
- **独占路径：** `crates/next-infra-connector-contract-tests/**`、`crates/next-infra-connector-catalog/**`、`fixtures/connectors/common/**`。
- **范围：** external ID、evidence、ordering、redaction、pagination、429、sync modes、errors、descriptor catalog。
- **非目标：** 无 UI、MCP serializer 或 runtime Connection Health。
- **输出：** 任意 Connector 可调用的 suite；逐 module 的 supported/partial/unsupported snapshot。
- **验收：** Catalog 包含 connector/version、module、resource/relation/schema、auth minimum、sync modes、known gaps、rate-limit guidance；不产生品牌级“已支持”。
- **验证：** descriptor invariant、catalog golden、Fixture conformance tests。
- **风险/停止：** MCP/UI 只能消费统一 Snapshot，不得各自重新解释 Descriptor。

### `CON-G2-05` — Connector Pipeline 集成证据

- **目标：** 注册 Fixture 并与 Normalizer/Sync/Store 证明 Coverage 和删除语义。
- **依赖：** `CON-G2-01..04`、`RHM-G2-03/04`。
- **独占路径：** `tests/integration/connector_pipeline/**`；registry/manifests 由 `GATE-G2` Captain 修改。
- **非目标：** 不实现 Provider。
- **验收：** partial/incremental/targeted/failed 不 tombstone；只有同 scope 连续两次成功 authoritative full 产生 tombstone；事务失败不前移 cursor。
- **验证：** Connector conformance + store/sync pipeline tests。
- **风险/停止：** 集成失败回派 API、Normalizer、Store 或 Sync owner。

并行波次：

```text
RHM-G2-01
  └─ CON-G2-01
       ├─ CON-G2-02
       ├─ CON-G2-03
       └─ CON-G2-04
              ↓
          CON-G2-05
```

## 3. Goal 5：GitHub / Actions

### `CON-G5-01` — GitHub Transport、认证与 Descriptor

- **目标：** 固定官方 API/SDK、细粒度只读权限、分页、ETag 和 rate-limit transport。
- **依赖：** `GATE-G4`、`CON-G2-05`。
- **独占路径：** GitHub crate 的 `lib/client/auth/descriptor/error` modules。
- **范围：** SecretProvider 临时取值、fake transport、module coverage shell。
- **非目标：** 不映射具体资源，不持久化 token。
- **输出：** 可供两个 mapper 分支使用的 GitHub Connector shell。
- **验收：** 429/reset、permission、pagination error 可 fake-test；凭据不进入 request summary。
- **验证：** GitHub transport/descriptor tests + common conformance。
- **风险/停止：** API/permission 易漂移；实施时只依赖官方文档并固定版本。

### `CON-G5-02` — Repository、Environment 与 Deployment

- **目标：** 映射 GitHub Repository、Environment、Deployment 及显式 Provider relations。
- **依赖：** `CON-G5-01`。
- **独占路径：** GitHub crate `repository/**`、`environment/**`、`deployment/**` 和专属 fixtures。
- **非目标：** 无 Workflow/Run/Job 或跨 Provider inference。
- **输出：** 稳定 Resource/Relation、module coverage。
- **验收：** visibility 只保留安全摘要；权限不足成为 module-level gap；fixture 无私有信息。
- **验证：** mapper golden + conformance tests。
- **风险/停止：** 不下载完整 deployment logs/payloads。

### `CON-G5-03` — Workflow、Run 与 Job

- **目标：** 映射 Repo → Workflow → Run → Job 观察与关系。
- **依赖：** `CON-G5-01`；可与 `CON-G5-02` 并行。
- **独占路径：** GitHub crate `actions/**` 和专属 fixtures。
- **非目标：** 不下载日志、artifact、环境变量或 Secret。
- **输出：** 有界 Actions resources/relations 和 pagination coverage。
- **验收：** 高频 Run/Job 有结果上限；partial page/429 不触发权威删除。
- **验证：** Actions mapper/pagination/conformance tests。
- **风险/停止：** 结果体积或 API scope 需要扩张时先回到 Descriptor owner。

### `CON-G5-04` — GitHub 纵切验收

- **目标：** 合并两个 mapper 并证明 Desktop/MCP 查询 Repo → Workflow → Run。
- **依赖：** `CON-G5-02/03`、`UI-G5-01` acceptance tests。
- **独占路径：** GitHub `tests/contract/**` 和 Goal 5 integration tests；registry/manifests 由 `GATE-G5` Captain 修改。
- **非目标：** 不在验收任务中修改 mapper、Query、UI 或保存 live response。
- **验收：** ETag、429、permission、partial pagination 不误删；真实只读账户只经 Keychain，且不录制响应。
- **验证：** GitHub conformance、vertical UI/MCP acceptance。
- **风险/停止：** 未配置真实只读身份时报告 blocked，不能用 Fixture 冒充 live acceptance。

## 4. Goal 6：SSH / Mac mini

### `CON-G6-01` — OpenSSH Transport 与 Probe Registry

- **目标：** 复用系统 OpenSSH，以固定 argv 调用版本化只读探针。
- **依赖：** `GATE-G5`、`DEC-G6-01`。
- **独占路径：** SSH crate `client.rs`、`descriptor.rs`、`probe_registry.rs`、`limits.rs`。
- **范围：** alias resolution、Host Key trust、timeouts/output limits、probe registration。
- **非目标：** 无任意命令文本、自动接受 Host Key 或展示名/IP 身份。
- **验收：** 用户输入不能进入 command body；Host Key verification 不可关闭；连接/探针/批次有界。
- **验证：** argv injection、host-key、timeout、descriptor tests + conformance。
- **风险/停止：** stable external ID 未冻结时不得开始 mapper。

### `CON-G6-02` — 通用主机探针

- **目标：** 收集 identity、uptime、filesystem 和 process summary。
- **依赖：** `CON-G6-01`。
- **独占路径：** SSH crate `probes/identity/**`、`uptime/**`、`filesystems/**`、`process_summary/**`。
- **非目标：** 不读取 env、history、任意文件或 secret directory。
- **验收：** 每个 probe 独立成功/失败；parser 对截断和未知 locale 安全失败。
- **验证：** fixed-output parser、timeout、truncation tests。
- **风险/停止：** 新命令必须先进入版本化 Probe Registry。

### `CON-G6-03` — macOS / Mac mini 探针

- **目标：** 提供 macOS host 与 launchd service 摘要。
- **依赖：** `CON-G6-01`；可与通用/Linux probes 并行。
- **独占路径：** SSH crate `probes/macos/**`。
- **非目标：** 不读取 launchd plist 内容、env、日志或任意文件。
- **输出：** `macos.launchd_services.v1` 等限定 schema。
- **验收：** 固定命令/解析器/最大输出；unreachable 不被解释为 host down。
- **验证：** synthetic macOS outputs、partial/error tests。
- **风险/停止：** 真实 Mac mini smoke 不得把 alias/IP/output 复制进 fixture。

### `CON-G6-04` — Linux systemd 探针

- **目标：** 提供 Linux host 与 systemd service 摘要。
- **依赖：** `CON-G6-01`；可与通用/macOS probes 并行。
- **独占路径：** SSH crate `probes/linux/**`。
- **非目标：** 不读取 unit file 内容、env、journal 或任意文件。
- **输出：** `linux.systemd_services.v1`。
- **验收：** 平台检测、parser 和 coverage 不复用 macOS 假设。
- **验证：** synthetic Linux outputs、partial/error tests。
- **风险/停止：** 未支持 init system 必须明确 unsupported，不猜测。

### `CON-G6-05` — SSH Security 与 Partial 纵切

- **目标：** 合并 probes 并证明安全边界及部分成功语义。
- **依赖：** `CON-G6-02..04`、`UI-G6-01`。
- **独占路径：** SSH `tests/security/**` 和 Goal 6 integration tests；registry 由 `GATE-G6` Captain 修改。
- **非目标：** 不在 QA 中改变 probe/transport，不保存真实主机输出。
- **验收：** Host Key mismatch 立即失败；timeout/truncation 可重放；一个 probe 失败仍提交其他观察且 run/coverage 为 partial；MCP/Tauri DTO 无命令字段。
- **验证：** SSH conformance/security/vertical acceptance。
- **风险/停止：** live alias 只用于本地 smoke，不进入提交或日志。

## 5. Goal 8：Dokploy 与 Cloudflare

### `CON-G8-01` — Dokploy Transport、Descriptor 与 DTO Allowlist

- **目标：** 建立只读 Dokploy client 和秘密字段不可达的 DTO 边界。
- **依赖：** `GATE-G7`、`DEC-G8-01`。
- **独占路径：** Dokploy crate base/client/auth/descriptor/DTO modules。
- **范围：** Project/Application/Deployment/Server/Domain transport 和 module coverage。
- **非目标：** Database 仅在 `DEC-G8-01` 明确扩围后才可加入；默认 `unsupported`。
- **验收：** password、connection string、token 不出现在可反序列化 DTO；未知字段丢弃。
- **验证：** allowlist/redaction/transport/conformance tests。
- **风险/停止：** 设计仍冲突时停止，不静默选 scope。

### `CON-G8-02` — Dokploy Resource Mapper

- **目标：** 映射 Project/Application/Deployment/Server/Domain 和显式 Provider relations。
- **依赖：** `CON-G8-01`。
- **独占路径：** Dokploy 各 resource module 和 fixtures。
- **非目标：** 不读取完整部署日志，不做跨 Provider inference。
- **验收：** stable identity/evidence；module-level coverage；敏感字段 sentinel 不存活。
- **验证：** mapper golden + conformance tests。
- **风险/停止：** Database scope 只能来自冻结后的 Descriptor。

### `CON-G8-03` — Cloudflare Transport、权限与 Descriptor

- **目标：** 建立资源限定 token 的只读 client 与逐模块 coverage。
- **依赖：** `GATE-G7`；可与 Dokploy 链并行。
- **独占路径：** Cloudflare crate base/client/auth/descriptor modules。
- **范围：** Account/Zone/DNS/Tunnel/Worker transport、pagination/rate-limit。
- **非目标：** 不保存 Worker code 或 token。
- **验收：** Account/Zone scope 和最小 Read permission 清晰；分页/限流可 fake-test。
- **验证：** transport/permission/conformance tests。
- **风险/停止：** 权限 scope 变动时以官方文档重新冻结。

### `CON-G8-04` — Cloudflare Resource Mapper

- **目标：** 映射 Account/Zone/DNS/Tunnel/Worker 摘要和显式 Provider relations。
- **依赖：** `CON-G8-03`。
- **独占路径：** Cloudflare 各 resource module 和 fixtures。
- **非目标：** 不下载 Worker code，不在 Connector 内做跨 Provider inference。
- **验收：** Worker 只保留摘要；权限缺失按 module 标记 partial/unsupported；DNS/route identity 稳定。
- **验证：** mapper golden + conformance tests。
- **风险/停止：** 不根据名称在 Connector 内创建跨平台 relation。

### `CON-G8-05` — 跨 Provider Topology Replay

- **目标：** 用合成数据重放 Repo → Deployment → Host → DNS。
- **依赖：** `CON-G8-02/04`、GitHub、SSH、Goal 7 Inference、`UI-G8-01`。
- **独占路径：** `tests/integration/topology/repo-deployment-host-dns/**`；registry 由 `GATE-G8` Captain 修改。
- **非目标：** 不修改 Provider mapper、Inference rules、Query 或 UI production code。
- **验收：** provider/configured/inferred evidence 不混淆；Connector 只供稳定字段；UI/MCP 均可追溯 relation 来源。
- **验证：** cross-provider replay、bounded topology、UI/MCP acceptance。
- **风险/停止：** inference rule files 仍由 Goal 7 owner 独占。

## 6. Goal 9：Supabase 与云厂商

Goal 9 入口可同时派发四条主线：Supabase managed、Supabase self-hosted source contract、Aliyun transport、Tencent transport。

### `CON-G9-S1` — Supabase Managed

- **目标：** 通过官方 Management API 只读收集 Organization/Project 摘要。
- **依赖：** `GATE-G8`。
- **独占路径：** `crates/next-infra-connector-supabase-managed/**` 及专属 fixtures。
- **非目标：** 不假定 self-hosted 具有相同 control plane，不默认要求高权限 token。
- **验收：** managed auth/fields/coverage 独立；Secret 只经 Keychain；permission gap 清晰。
- **验证：** managed conformance + fake transport tests。
- **风险/停止：** API/permission 必须按实施时官方文档确认。

### `CON-G9-S2` — Supabase Self-hosted Source Contract

- **目标：** 冻结 service API、PostgreSQL metadata、container summary 和固定 SSH probe 的组合边界。
- **依赖：** `GATE-G8`、SSH Probe Registry。
- **独占路径：** self-hosted crate descriptor/source-adapter modules。
- **非目标：** 无任意 container command、env export 或 managed DTO reuse。
- **验收：** 每种 source 单独声明 Coverage/error；凭据和容器环境变量不可进入 Observation。
- **验证：** source adapter contract/redaction tests。
- **风险/停止：** 不得绕过固定 SSH probe 边界。

### `CON-G9-S3` — Supabase Self-hosted Normalization

- **目标：** 映射 self-hosted service/database/runtime 摘要。
- **依赖：** `CON-G9-S2`。
- **独占路径：** self-hosted mapper modules 和 fixtures。
- **非目标：** 不复用 managed DTO，不运行任意 SSH/container command。
- **验收：** 任一 source 不可用时保留其他观察并返回 partial；managed/self-hosted identity 不混合。
- **验证：** multi-source partial/conformance tests。
- **风险/停止：** 不读取完整 database config、container env 或 connection string。

### `CON-G9-A0` — Aliyun Transport 与 Descriptor

- **目标：** 建立独立只读 RAM identity、region transport 和 module shell。
- **依赖：** `GATE-G8`。
- **独占路径：** Aliyun crate base/auth/client/descriptor modules。
- **非目标：** 不映射具体资源，不实现写 API，不保存 Access Key/Secret Key。
- **验收：** region、pagination、signature error、rate-limit 可 fake-test；module coverage 分列。
- **验证：** Aliyun transport/conformance tests。
- **风险/停止：** 不以“Aliyun supported”替代具体 module 结论。

### `CON-G9-A1` — Aliyun Compute

- **目标：** 映射 ECS instance 和公网地址摘要。
- **依赖：** `CON-G9-A0`。
- **独占路径：** Aliyun `compute/**`。
- **非目标：** 不映射 VPC/LB/DNS，不创建跨 Provider inference。
- **验收：** stable instance identity；临时公网 IP 不单独成为高置信跨 Provider relation。
- **验证：** compute mapper/conformance tests。

### `CON-G9-A2` — Aliyun Network

- **目标：** 映射 VPC、Subnet、Security Group 和显式 Provider relations。
- **依赖：** `CON-G9-A0`；可与 A1/A3 并行。
- **独占路径：** Aliyun `network/**`。
- **非目标：** 不映射 Compute/Edge，不把规则明细保存为原始响应。
- **验收：** 产品模块独立 Coverage；region partial 不伪装 full。
- **验证：** network mapper/conformance tests。

### `CON-G9-A3` — Aliyun Edge

- **目标：** 映射 Load Balancer、Public IP、DNS 摘要。
- **依赖：** `CON-G9-A0`；可与 A1/A2 并行。
- **独占路径：** Aliyun `edge/**`。
- **非目标：** 不映射 Compute/VPC，不按名称猜跨 Provider relation。
- **验收：** edge submodules 分列；未覆盖产品明确 unsupported。
- **验证：** edge mapper/conformance tests。

### `CON-G9-T0` — Tencent Transport 与 Descriptor

- **目标：** 建立独立只读 identity、region transport 和 module shell。
- **依赖：** `GATE-G8`；可与 Supabase/Aliyun 主线并行。
- **独占路径：** Tencent crate base/auth/client/descriptor modules。
- **非目标：** 不映射具体资源，不实现写 API，不保存 SecretId/SecretKey。
- **验收：** region、pagination、signature error、rate-limit 可 fake-test；module coverage 分列。
- **验证：** Tencent transport/conformance tests。
- **风险/停止：** 不以“Tencent supported”替代具体 module 结论。

### `CON-G9-T1` — Tencent Compute

- **目标：** 映射 CVM instance 和公网地址摘要。
- **依赖：** `CON-G9-T0`。
- **独占路径：** Tencent `compute/**`。
- **非目标：** 不映射 VPC/LB/DNS，不创建跨 Provider inference。
- **验收：** stable identity；动态 IP 不单独成为高置信 relation。
- **验证：** compute mapper/conformance tests。

### `CON-G9-T2` — Tencent Network

- **目标：** 映射 VPC、Subnet、Security Group 和 Provider relations。
- **依赖：** `CON-G9-T0`；可与 T1/T3 并行。
- **独占路径：** Tencent `network/**`。
- **非目标：** 不映射 Compute/Edge，不保存原始安全组响应。
- **验收：** module coverage 和 region partial 明确。
- **验证：** network mapper/conformance tests。

### `CON-G9-T3` — Tencent Edge

- **目标：** 映射 Load Balancer、Public IP、DNS 摘要。
- **依赖：** `CON-G9-T0`；可与 T1/T2 并行。
- **独占路径：** Tencent `edge/**`。
- **非目标：** 不映射 Compute/VPC，不按名称猜跨 Provider relation。
- **验收：** edge submodules 分列；未覆盖产品明确 unsupported。
- **验证：** edge mapper/conformance tests。

### `CON-G9-04` — Coverage Matrix 集成

- **目标：** 汇合所有 Goal 9 descriptor fragment，产生统一、逐 module 的 Connector Coverage。
- **依赖：** `CON-G9-S1/S3/A1..A3/T1..T3`。
- **独占路径：** Goal 9 connector registry、provider contract snapshots、coverage integration tests；本任务不修改各 Provider mapper。
- **非目标：** 不运行 live account validation，不生成品牌级 supported 布尔值。
- **验收：** managed/self-hosted 分列；Aliyun/Tencent 每个产品分列；Connector Coverage、Sync Coverage、Connection Health 不混合。
- **验证：** Catalog golden、permission/rate/region adversarial tests；UI 联合验证仅在 `GATE-G9` 执行。
- **风险/停止：** Provider worker 只提交 descriptor fragment；本任务单写注册避免冲突。

### `CON-G9-05` — 真实只读账户验证

- **目标：** 验证权限范围、region pagination、partial 和 rate-limit，且不保存真实响应。
- **依赖：** `CON-G9-04`；用户已配置对应只读 SecretRef。
- **独占路径：** 清洗后的 acceptance scripts/results template；不保存 response fixtures。
- **非目标：** 不创建/修改 Provider 资源，不扩大权限，不修改用户凭据配置。
- **输出：** 只有计数、状态分类、耗时、coverage gap 的验证报告。
- **验收：** 每个已配置 Provider 的真实路径可证明；未配置时明确 `credential_unavailable/BLOCKED`。
- **验证：** Provider-specific live read-only commands，以实施时官方文档为准。
- **风险/停止：** 绝不把 Fixture 通过写成 live account 通过；不打印 account ID、repo、host、IP 或 response body。
