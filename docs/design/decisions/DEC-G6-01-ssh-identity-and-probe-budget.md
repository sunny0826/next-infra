# DEC-G6-01：SSH 稳定身份与探针预算

- **状态：** Accepted for Goal 6 implementation
- **日期：** 2026-08-06
- **关联任务：** `CON-G6-01`、`CON-G6-02`、`CON-G6-03`、`CON-G6-04`、`CON-G6-05`
- **适用范围：** SSH / Mac mini Connector 的只读 transport、Probe Registry、配置和错误边界
- **不构成：** 工程实现、真实主机连接、SSH 配置写入、任意命令、MCP/Agent 配置或 Apple 身份授权

本文是 Goal 6 的 SSH 身份、信任、预算和 registry 权威决策。实现必须遵守 [`implementation-goals.md`](../implementation-goals.md) 的 Goal gate、[`connector-and-sync-contract.md`](../connector-and-sync-contract.md) 第 6 节、[`agent-interface-and-security.md`](../agent-interface-and-security.md) 第 10 节以及 [`resource-and-storage-model.md`](../resource-and-storage-model.md) 的资源身份和同步语义。共享错误和 ID 类型分别见 [`error.rs`](../../../crates/next-infra-core/src/error.rs) 与 [`ids.rs`](../../../crates/next-infra-core/src/ids.rs)。任务调度和 Goal gate 见 [`docs/tasks/README.md`](../../tasks/README.md) 与 [`docs/tasks/connectors.md`](../../tasks/connectors.md)。

## 1. 决策摘要

| 事项 | 冻结结论 |
| --- | --- |
| Host 稳定身份 | 配置创建时生成一次、之后不可变的 opaque `host_identity`；不得由 alias、hostname、IP、用户名、端口或 Host Key 推导 |
| `ssh.host` external ID | `ssh-host:v1:<host_identity>`；`host_identity` 变更意味着新资源，不是 rename |
| Alias | 只作为 OpenSSH config 的 locator；必须是具体、受限的 alias，不是身份或展示名 |
| Hostname / IP | 可由用户已有 SSH alias 解析并用于连接，但永远不是身份；Next Infra 配置不接受它们作为覆盖字段 |
| Host Key | 系统 OpenSSH 的信任证据；固定 `StrictHostKeyChecking=yes`。不匹配立即失败，不接受、不重试、不旋转 `host_identity` |
| Transport | 直接执行 `/usr/bin/ssh` 的固定 argv，不经过 shell；公开 API 只接受已注册 `ProbeId`，不接受命令或 argv |
| SSH config | 可继续由系统 OpenSSH 使用 alias、ProxyJump、SSH Agent、IdentityFile；CLI 安全选项不能被用户配置削弱 |
| 预算 | 单 Connection 并发 1、单批最多 6 probes、单批 90 s、单 probe 20 s、单批输出 2 MiB，并受连接/单 probe 流预算约束 |
| 验收边界 | fake executor 和合成输出足以完成当前 transport/registry 验收；真实 SSH、MCP 和 Apple identity 不是当前前置条件 |

## 2. 身份与资源键

### 2.1 `host_identity` 的生成和生命周期

每个 SSH Connection 在创建时生成一个密码学随机的 128-bit UUID v4，使用小写 canonical 文本（36 个 ASCII 字符，格式 `8-4-4-4-12`）。它是 opaque 标识，不承载平台、位置、租户或时间语义。生成必须发生在 Connection 配置持久化之前，且必须和配置一起保存；实现不得在每次同步时重新生成。

以下值都禁止参与生成、回退或重写 `host_identity`：

- `host_alias`、hostname、IP、端口、用户名和 display name；
- SSH Host Key、fingerprint、known_hosts 路径和 SSH config 文件内容；
- 远程 `uname`、machine-id、序列号、环境变量或任何 probe 输出；
- 将两个 Connection “看起来相同”的启发式合并结果。

导入缺少 identity 的旧配置时只允许执行一次“生成并保存”；不能用 alias 或地址填充。已有 identity 的配置若格式非法或与另一个 SSH Connection 冲突，应在连接前返回结构化配置错误，不得静默改写。删除并重新创建 Connection 会得到新的 identity，即使它再次使用同一 alias。

### 2.2 Stable external ID

Host 资源的 kind 固定为 `ssh.host`，external ID 固定为：

```text
ssh-host:v1:<host_identity>
```

`v1` 是 external ID 格式版本，不是 OpenSSH 或 Host Key 版本。`Resource` 的唯一键仍包含 `connection_id`；两个 Connection 即使暂时使用相同 alias，也不会自动合并。相同 alias 的两个配置必须使用不同的生成 identity；若显式导入相同 identity，配置层返回 `Conflict` 并停止。

示例（均为合成值）：

| 变化 | 预期结果 |
| --- | --- |
| alias 从 `mac-mini` 改为 `mini-lab`，解析地址不变 | external ID 不变 |
| alias 不变，但 SSH config 解析到新的 hostname/IP | 仍使用旧 external ID；Host Key 校验决定是否允许连接 |
| 同一 alias 创建第二个 Connection | 新生成 identity，产生不同 external ID |
| Host Key 变更或校验失败 | 原 external ID 保留；本轮 fatal，不生成替代 ID |
| 仅 display name 变化 | external ID 不变 |

展示名、alias、解析出的 hostname/IP 和 trust 状态是独立字段；它们不能作为 `ExternalId` 的替代值，也不能用于跨 Connection 自动合并。

## 3. 配置 schema v1

Connector descriptor 的 `config_schema_version` 固定为 `1`。其配置对象为 `SshConnectionConfigV1`，只包含非秘密字段：

```text
host_identity: string
host_alias: string
connect_timeout_secs?: u8
probe_profile: registered ProbeProfile
allowed_service_ids?: string[]
```

| 字段 | 约束和默认值 |
| --- | --- |
| `host_identity` | 必填；小写 UUID v4 canonical 文本；创建后不可变；不得由调用方用 alias/地址替代 |
| `host_alias` | 必填；匹配 `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`；最长 128 bytes；拒绝前导 `-`、空白、控制字符、glob、`/`、`:` 和 shell metacharacters |
| `connect_timeout_secs` | 可选，缺省 `10`；有效范围 `1..=10`；调用方可以收紧，不能放宽 |
| `probe_profile` | 必须是 Probe Registry 已注册的 profile；首版只接受 `baseline-v1`，不接受自由文本或命令片段 |
| `allowed_service_ids` | 可选，最多 64 项；每项最多 128 ASCII bytes，使用固定安全字符集且重复项拒绝；它是用户显式声明的远程 service label allowlist，只由 launchd/systemd parser 本地筛选，不进入命令文本 |

配置解析使用显式 allowlist，未知字段和以下字段均拒绝：`hostname`、`ip`、`port`、`username`、`known_hosts` 路径、`IdentityFile` 内容或路径、`ProxyCommand`、`shell`、`command`、`args`、环境变量、私钥、口令和 Secret。用户已有 SSH config 仍可由系统 OpenSSH 为已验证 alias 提供 `ProxyJump`、SSH Agent 和 `IdentityFile`；这些不是 Next Infra 配置字段，也不由 Next Infra 复制、解析或修改。

`host_alias` 必须先完整校验，再允许 transport 构造 child。任何校验失败都在本地返回 `InvalidDomainValue`，不启动 `/usr/bin/ssh`。Debug、Display、serde failure 和 request summary 只显示字段类别与结构化原因，不回显配置值。

## 4. OpenSSH transport

### 4.1 固定执行方式

生产 transport 等价于 `Command::new("/usr/bin/ssh")`，直接传递下列固定 argv（顺序也是 contract）：

```text
-T
-o BatchMode=yes
-o StrictHostKeyChecking=yes
-o ConnectionAttempts=1
-o ConnectTimeout=<validated 1..=10>
-o NumberOfPasswordPrompts=0
-o RequestTTY=no
-o LogLevel=ERROR
-o AddKeysToAgent=no
-o ClearAllForwardings=yes
-o ControlMaster=no
-o ControlPath=none
-o ControlPersist=no
-o ForwardAgent=no
-o ForwardX11=no
-o PermitLocalCommand=no
-o UpdateHostKeys=no
<validated host_alias>
<registry-owned fixed remote command>
```

规则如下：

1. 不设置 `shell=true`，不调用 `/bin/sh -c`，不通过字符串拼接生成本地命令。
2. remote command 只能来自编译期 Probe Registry 的固定 entry；用户、UI、MCP、配置和 provider 内容都不能提供或修改它。
3. 所有固定 `-o` 均位于 alias 之前，使安全约束覆盖用户 SSH config 的同名设置。transport 不得加入或接受 `StrictHostKeyChecking=no`、`StrictHostKeyChecking=accept-new`、`UserKnownHostsFile=/dev/null` 或其他削弱验证的 CLI 选项；固定关闭 forwarding、LocalCommand、ControlMaster、Agent/X11 forwarding、known-host 自动更新和自动加入 Agent。
4. 不以 CLI 参数覆盖用户 alias 对 `ProxyJump`、SSH Agent、IdentityFile 的合法配置；但如果用户配置与固定安全约束冲突，连接失败而不是降级。
5. stdin 固定为 null；stdout/stderr 通过独立的有界 pipe 流式读取，不继承交互终端，不弹出密码提示。本地 child 固定 `LC_ALL=C`、`LANG=C` 与 `SSH_ASKPASS_REQUIRE=never`，并移除 `SSH_ASKPASS`；其余环境保留，以便系统 OpenSSH 使用当前用户的 `HOME` 与 `SSH_AUTH_SOCK`。
6. cancel、timeout 或任一流超限只终止本次精确 child，并等待该 child reaped；禁止按进程名或广域 kill。

公开 API 只接受 `ProbeId`、连接配置和已注册的 service filter。不存在 `run_command`、terminal、shell、sudo、root、文件读取、日志读取、端口转发或任意 argv setter。

### 4.2 用户 SSH config 的边界

系统 OpenSSH 可以读取用户已有 alias 和其合法的 `ProxyJump`、SSH Agent、IdentityFile 设置。Next Infra 不读取、写入、迁移或重排 `~/.ssh/config`、known_hosts、Agent socket 或私钥。connector 不能以配置字段传入 hostname/IP/port/username 来绕过 alias，也不能为一次 probe 临时写配置文件。

固定 argv 必须始终保留 `BatchMode=yes`、`StrictHostKeyChecking=yes`、`ConnectionAttempts=1`、`NumberOfPasswordPrompts=0`、`RequestTTY=no` 以及上述本地副作用禁用项。用户配置不能将这些保护降级；如果系统 OpenSSH 仍无法建立满足约束的连接，按第 8 节分类。

## 5. Host Key 信任语义

Host Key 是系统 OpenSSH 的信任证据，不是 Next Infra 的资源身份。首次建立信任由用户在系统 SSH 环境中完成；connector 不运行 `ssh-keyscan`，不自动写 known_hosts，不接受 `accept-new`，也不把 fingerprint 当作 `host_identity`、external ID 或 alias。

`StrictHostKeyChecking=yes` 下出现 Host Key changed、未知 key 或 verification failure，必须：

- 立即返回 `ErrorCode::HostKeyMismatch`，`retryable = false`；
- 使当前 SSH sync fatal，不能提交本轮其他 probe 观察；
- 不自动 retry、旋转 identity、替换 known_hosts、切换地址或把旧资源标为 `unhealthy`；
- 丢弃原始 stderr、fingerprint、alias、hostname、IP、用户名和 known_hosts 路径，仅保留固定的清洗后错误消息。

如果用户确认这是新的主机，应创建新的 Connection 并生成新的 `host_identity`；不能通过修改 alias 或 Host Key 让旧 external ID 静默指向另一台主机。Host Key 的 `verified`/`mismatch` 只可作为本轮 trust evidence 和 Connector Health 的安全状态，不得成为 Resource identity。

## 6. 版本化 Probe Registry

首版 registry 只注册以下六个 ID。每个 entry 私有持有固定 schema version、platform、remote command、timeout、stdout/stderr limit 和 parser owner；外部 metadata 只能按 ID 返回非命令信息。

| ProbeId | 支持平台 | 默认 wall timeout | stdout 上限 | 目的 |
| --- | --- | ---: | ---: | --- |
| `host.identity.v1` | all | 10 s | 64 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -s; LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -m` |
| `host.uptime.v1` | all | 10 s | 16 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uptime` |
| `host.filesystems.v1` | all | 15 s | 256 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin df -Pk` |
| `host.process_summary.v1` | all | 15 s | 256 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin ps -Ao state=,comm=` |
| `macos.launchd_services.v1` | macOS | 20 s | 512 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin launchctl list` |
| `linux.systemd_services.v1` | Linux | 20 s | 512 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin systemctl list-units --type=service --all --no-pager --no-legend --plain` |

所有 probe 的 stderr 上限固定为 32 KiB。单 probe 的 registry timeout 和 stdout limit 只能低于或等于全局预算；不能由 UI、MCP、Connection config 或调用方提高。service filter 只接受配置中经过语法和数量验证的显式 allowlist，并由后续 parser 匹配远程 service label；它不进入 command。

Registry 规则：

- ID 不重复，ID 与输出 schema 版本绑定；同一 ID 不得改变 remote command 或字段语义。
- 修改 command、platform、输出 schema 或清洗边界时必须创建新的 versioned `ProbeId`，并经过安全/契约 Review；不能在实现中热替换字符串。
- 上表命令文本是对应 ProbeId 的最终 v1 合同，`CON-G6-01` 必须按字节固定；transport 测试若需要无网络执行，只能使用 `cfg(test)` 私有的 test-only entry，不得用占位 command 注册生产 ProbeId。尚未实现的 parser/module 不能因此标记为 `Supported`。
- registry metadata 不暴露 command、路径、环境、秘密或远程响应；未知 ProbeId 在 spawn 前失败。

## 7. 连接、probe、输出和批次预算

以下是不可由调用方放宽的硬上限：

| 预算 | 硬上限 |
| --- | ---: |
| 同一 SSH Connection 并发 child | 1 |
| 单批 Probe 数 | 6 |
| `ConnectTimeout` | `1..=10 s`（默认 10 s） |
| 单 Probe wall time | 20 s |
| 单批 wall time | 90 s |
| 单 Probe stderr | 32 KiB |
| 单批 stdout + stderr | 2 MiB |

transport 在 spawn 前同时检查 registry entry、剩余批次时间和剩余输出预算；剩余时间成为当前 probe 的更低有效 timeout，剩余时间为零或无法预留该 probe 最大输出时不启动新的 child。一个 probe 的较低 registry timeout/输出上限优先于全局上限。达到任一 wall/output 上限时立即停止当前精确 child、等待 reaped，并返回该 probe 的结构化结果；不得继续累积超限 bytes 或把超时伪装成成功。

批次最多包含六个 registry entry，每个 Connection 同时只允许一个 child。单批预算耗尽时不再 spawn 后续 probe。批次输出预算统计所有已完成和当前 probe 的 stdout/stderr bytes；stderr 永不进入 Resource attributes。批次超时可形成 retryable `ProviderUnavailable`，但不能绕过 Host Key fatal 规则或 tombstone 语义。

## 8. 错误分类与部分成功

transport 复用 Core 的 `ErrorCode` 和 Connector `ConnectorFailure`，不得创建平行错误枚举。公开 message 是固定短文本，必须清洗 Token、Authorization、Cookie、密码、私钥路径、alias、地址、账户、known_hosts 路径和远程正文。

| 条件 | `ErrorCode` | retryable | 说明 |
| --- | --- | --- | --- |
| alias、配置、ProbeId 或 profile 无效 | `InvalidDomainValue` | false | 本地校验失败，不 spawn |
| 同一 identity 已被另一个 SSH Connection 占用 | `Conflict` | false | 不猜测、不自动生成替代值 |
| 本地 `/usr/bin/ssh` 缺失或无法启动 | `Internal` | false | 不回显 executable/path 细节 |
| Host Key changed / verification failed | `HostKeyMismatch` | false | 当前 sync fatal；不接受、不旋转 |
| publickey/agent/permission authentication failure | `AuthenticationFailed` | false | 不把系统密码提示当作 fallback |
| DNS、route、refused、connect timeout | `NetworkUnreachable` | true | 影响 Connector Health/Freshness，不把主机伪装为 down |
| probe 或 batch wall timeout | `ProviderUnavailable` | true | 当前 child 被终止并 reaped |
| stdout/stderr 或批次输出超限 | `InvalidResponse` | false | 丢弃超限响应 |
| remote command non-zero 且无法安全细分 | `ProviderUnavailable` | false | 仅保留固定分类 |
| caller cancellation | `Cancelled` | false | 停止当前 child，不扩大 kill 范围 |

分类器只能匹配有限、固定的 OpenSSH error signatures；无法安全细分的退出码不能被解释成认证成功或 Host Key 成功。`CredentialUnavailable` 等 Keychain/SecretProvider 语义不由本 transport 产生，不能新增另一套 SSH error code。

单 probe 的 timeout、output overflow、非零退出或 parser failure，不应丢弃同批已经成功的其他 observations；上层 connector 可将这种结果标为 `partial`。没有任何有效 observation 时，按 Connector contract 返回 fatal。Host Key mismatch 是例外：无论此前已有多少成功 probe，都使整个 sync fatal，不能提交部分观察。partial、网络失败和取消都不得增加缺失计数或触发 tombstone。

## 9. Descriptor 与当前覆盖

`ssh` Connector descriptor 固定如下：

- `connector_type = "ssh"`；`connector_version = "1.0.0"`；`config_schema_version = 1`。
- `auth.kind = SshAgent`；minimum permission 只描述用户已有 SSH alias 与非 root 只读 probe 权限，不声明或保存 Keychain Secret。
- 支持 `Full` 与 `Targeted`；Targeted 只定位 `ssh.host` 的稳定 external ID，不能携带 command、argv 或地址。
- 默认 concurrency `1`，recommended interval `300 s`；调度不能突破本决策硬上限。
- `ssh.host`、filesystem、process、launchd 和 systemd resource capabilities 在对应 mapper (`CON-G6-02/03/04`) 完成前保持 `Partial`，不能用空 batch 冒充已采集。
- known gaps 至少包括：无任意命令、无自动 Host Key 接受、不读取 env/history/files/secrets/logs、默认非 root、无 Windows/非-systemd init 支持，以及 live alias 未验证。

远程不可达只改变 Connector Health/Freshness；既有 Resource 保留最后已知 health 和 `observed_at`，不能因本次 transport 失败直接写成 `unhealthy`。

## 10. 自动化验收

实现必须以 fake executor 和合成输出覆盖：

1. alias 合法边界，以及 `-oProxyCommand`、空白、newline、glob、slash、colon 和 shell metacharacter 注入拒绝。
2. `host_identity` 生成一次、alias rename、hostname/IP 变化、同 alias 不同 identity、重复 identity conflict 和 Host Key mismatch 不旋转 identity。
3. `/usr/bin/ssh`、argv 精确顺序、null stdin、无 shell、无用户 command/argv setter，以及固定安全 `-o` 不能被配置削弱。
4. Host Key mismatch、认证失败、网络失败、未知 255、普通 non-zero 的结构化分类、retryable 值和清洗结果。
5. stdout/stderr 分别超限、单 probe timeout、batch timeout、cancel 时精确 child 被终止并 reaped。
6. 单 Connection 并发、六 probe 上限、`1..=10 s` connection timeout、20/90 s wall budgets、32 KiB/2 MiB output budgets；剩余预算不足时不 spawn。
7. Registry 无重复 ID；六个 entry 的平台、版本、timeout、stdout/stderr 上限固定；metadata 不暴露 remote command。
8. `SshConnectionConfigV1` unknown-field/秘密字段拒绝，descriptor/conformance 通过，mapper pending module 不标记 `Supported`。
9. Debug、Display、serde failure、request summary、ConnectorFailure 和测试快照不包含 alias、hostname、IP、用户名、路径、fingerprint 或远程 output sentinel。
10. dependency closure 不引入 Store、Sync、Runtime、Query、Tauri、MCP、Keychain、GitHub 或第三方 SSH protocol implementation。

当前决策不要求或执行以下验收：真实 SSH alias/host connection、真实 known_hosts 或 Agent、Codex/Hermes MCP 配置、Apple Development/Developer ID identity、Keychain smoke、签名、公证和发布。它们不是 Goal 6 当前 transport/registry acceptance 的前置条件。

## 11. 非目标与停止规则

明确非目标：

- 不实现 Resource mapper、Normalizer/Store/Sync integration、Runtime registry、UI 或 MCP 工具。
- 不读取或修改真实 SSH config、known_hosts、Agent、IdentityFile 或远程输出；不保存真实主机、hostname、IP、fingerprint 或 provider response fixture。
- 不增加 `run_command`、terminal、shell、sudo、root、文件/环境/日志/秘密读取或端口转发能力。
- 不把 Host Key fingerprint、alias、hostname 或 IP 当作 `external_id`，不因 Host Key mismatch 自动换 identity。

实现若需要下列任一变化，必须立即停止并回派 Decision/Gate Captain，不得在 SSH crate 内顺手解决：

1. 修改 `ErrorCode`、`ExternalId`、`Resource`/`Connection` identity、ConnectorFailure、Sync partial/tombstone 或任何共享 Core/Connector API。
2. 接受动态 remote command、解析/写入 SSH config、放宽 Host Key 验证、绕过 alias 校验或提高任一预算。
3. 新增未版本化 ProbeId、改变既有 probe 的 command/schema 语义，或把 pending module 标为 Supported。
4. 让 Tauri/MCP/Agent 传入 command、argv、hostname/IP、Secret 或触发外部写操作。
5. 以真实主机、MCP 用户配置或 Apple signing/Keychain identity 作为本地 transport 单元测试的隐含前置条件。

本决策冻结的是可验证的设计边界，不授权任何外部状态写入。若上游共享契约无法满足上述边界，状态保持 `BLOCKED`，不能通过降低安全约束或扩大 scope 继续实现。
