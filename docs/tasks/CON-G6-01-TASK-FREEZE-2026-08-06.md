# CON-G6-01 OpenSSH Transport 与 Probe Registry 任务冻结

**冻结日期：** 2026-08-06  
**状态：** `FROZEN / IMPLEMENTED / REVIEW`  
**受众：** SSH Connector owner、后续 Probe owner、Goal 6 Gate Captain  
**入口授权：** `GATE-G5` 内部轨道已通过；真实 SSH host、MCP、Apple Development 与 Developer ID 验收不属于本任务。

## 1. 目标与完成定义

本任务只建立系统 OpenSSH transport、版本化 Probe Registry、预算和错误边界。完成后，`CON-G6-02/03/04` 可以按注册的 `ProbeId` 实现通用、macOS 和 Linux parser；本任务本身不生成 `ResourceObservation`，也不连接真实主机。

完成必须同时满足：

1. 新建独立 `next-infra-connector-ssh` crate；生产依赖只指向 `next-infra-core` 与 `next-infra-connector-api`，不依赖 Store、Sync、Runtime、Query、Tauri、MCP、Keychain 或 GitHub Connector。
2. 只执行本 crate 注册的固定探针。公开 API 接受 `ProbeId`，不接受命令、脚本、路径或任意 argv。
3. 只以独立 argv 启动固定 `/usr/bin/ssh`，不经过 shell；SSH alias 必须先通过本文件的语法校验。
4. 命令行强制 BatchMode、Host Key 验证、无 TTY、单次连接尝试和有界超时；用户配置不能关闭这些保护。
5. stdout、stderr、单探针 wall time、单连接批次时间和批次总输出都有硬上限；超限立即终止精确子进程并返回清洗后的结构化错误。
6. `HostKeyMismatch` 独立于网络不可达和认证失败，非 retryable；任何输出、错误或 Debug 都不保留 alias、hostname、IP、用户名、known_hosts 路径或远程正文。
7. fake executor 可以确定重放 argv、成功、退出码、Host Key mismatch、认证失败、网络失败、timeout、cancel 和输出超限，不运行真实 SSH。
8. Descriptor 只声明 transport/probe registry 已就绪；尚未实现的资源模块保持 `Partial`，不能用空 batch 冒充 SSH 已采集。

## 2. 冻结输入

实施必须遵守：

- [`DEC-G6-01`](../design/decisions/DEC-G6-01-ssh-identity-and-probe-budget.md) 的稳定 `host_identity`、alias 角色、信任规则与预算。
- [`connector-and-sync-contract.md`](../design/connector-and-sync-contract.md) 第 6 节的系统 OpenSSH 与固定探针边界。
- [`agent-interface-and-security.md`](../design/agent-interface-and-security.md) 第 10 节的无任意命令、无 Host Key 绕过和输出限制。
- 现有 `ConnectorDescriptor`、`ConnectorFailure` 与 `ErrorCode::HostKeyMismatch`；本任务不得新增平行错误体系。

本机基线是 `/usr/bin/ssh` 的 `OpenSSH_10.2p1`。这只证明开发机可执行文件存在，不构成远程连通性或 Host Key 验收。

## 3. Crate 与所有权

Feature Owner 独占：

```text
crates/next-infra-connector-ssh/
  Cargo.toml
  src/
    lib.rs
    client.rs
    descriptor.rs
    error.rs
    limits.rs
    probe_registry.rs
  tests/
    transport_contract.rs
```

Gate Captain 独占根 `Cargo.toml`、`Cargo.lock`、dependency guard 和 workspace package-count 更新。Feature Owner 不注册 Runtime、Desktop、MCP 或 Connector Catalog。

允许依赖：

- `next-infra-core`、`next-infra-connector-api`。
- `async-trait`、`serde`、`serde_json`。
- `tokio` 的 `process`、`time`、`io-util` 能力，用于有界子进程和流式输出；不允许 `wait_with_output` 无界收集。
- `uuid = 1.24.0` 的 `v4` 能力，仅用于生成和校验 opaque `host_identity`；不从 alias、地址或 Host Key 派生。

不引入 SSH 协议 crate，不读取或重写 `~/.ssh/config`、`known_hosts`、SSH Agent socket 或私钥。

## 4. 配置与输入边界

`SshConnectionConfigV1` 只包含非秘密字段：

```text
host_identity
host_alias
connect_timeout_secs
probe_profile
allowed_service_ids
```

- `host_identity` 的生成、稳定性和禁止推断规则由 `DEC-G6-01` 定义。
- `host_alias` 是 OpenSSH config 的 transport locator，不是 Resource identity。必须匹配 `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`；拒绝前导 `-`、空白、control、通配符、斜杠、冒号和 shell metacharacters。
- `connect_timeout_secs` 必须在 `1..=10`；缺失时为 10。调用方只能收紧，不能放宽。
- `probe_profile` 首版只接受 Registry 固定枚举；`allowed_service_ids` 是数量与字符集受限的用户显式 service label allowlist，只由后续 parser 本地筛选，不进入命令文本。
- config 不接受 hostname、IP、port、username、IdentityFile、known_hosts path、ProxyCommand、shell、command、args 或环境变量覆盖。需要这些能力时由用户已有 SSH alias 配置承担。

`Debug`、validation error 和 serialized failure 只能显示字段分类与结构化原因，不回显上述值。

## 5. 固定进程合同

生产 transport 使用 `Command::new("/usr/bin/ssh")`，固定 argv 顺序为：

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

规则：

- 不设置 `shell=true`，不调用 `/bin/sh -c` 生成本地命令。
- remote command 是 `ProbeRegistry` 内的编译期静态值；所有动态字段均不得拼接或格式化进命令。
- 固定 `-o` 必须位于 alias 前，使其覆盖用户 SSH config 中同名选项；不得加入 `StrictHostKeyChecking=no|accept-new`、`UserKnownHostsFile=/dev/null` 或其他削弱验证的选项。必须固定关闭 forwarding、LocalCommand、ControlMaster、Agent/X11 forwarding、known-host 自动更新和自动加入 Agent。
- 允许系统 OpenSSH 自行读取用户已有 alias、ProxyJump、SSH Agent 与 IdentityFile；Next Infra 不解析、复制或记录这些配置。
- stdin 固定为 null，stdout/stderr 分别以有界 pipe 读取；不得继承交互终端或弹出密码提示。本地 child 固定 `LC_ALL=C`、`LANG=C`、`SSH_ASKPASS_REQUIRE=never` 并移除 `SSH_ASKPASS`，其余环境保留供系统 OpenSSH 使用当前用户的 `HOME` 与 `SSH_AUTH_SOCK`。
- cancel、timeout 或任一流超限时只终止当前精确 child；清理必须等待 child reaped，不使用进程名匹配或广域 kill。

## 6. Probe Registry 合同

Registry 首版只注册以下 ID：

| ProbeId | 支持平台 | 默认超时 | stdout 上限 | 目的 |
|---|---|---:|---:|---|
| `host.identity.v1` | all | 10 s | 64 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -s; LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uname -m` |
| `host.uptime.v1` | all | 10 s | 16 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin uptime` |
| `host.filesystems.v1` | all | 15 s | 256 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin df -Pk` |
| `host.process_summary.v1` | all | 15 s | 256 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin ps -Ao state=,comm=` |
| `macos.launchd_services.v1` | macOS | 20 s | 512 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin launchctl list` |
| `linux.systemd_services.v1` | Linux | 20 s | 512 KiB | `LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin systemctl list-units --type=service --all --no-pager --no-legend --plain` |

所有探针 stderr 上限为 32 KiB。Registry entry 必须包含固定 ID、schema version、platform、remote command、timeout、stdout/stderr limit 和 parser ownership；字段私有，外部只能按 ID 查询非命令 metadata。

上表命令文本是对应 ProbeId 的最终 v1 合同，`CON-G6-01` 必须按字节固定。transport 测试若需要无网络执行，只能使用 `cfg(test)` 私有 test-only entry，不得用占位 command 注册生产 ProbeId。新增/修改生产 command 必须使用新的版本化 ProbeId，并经过安全 Review。

## 7. 批次预算

| 预算 | 硬上限 |
|---|---:|
| 同一 SSH Connection 并发 child | 1 |
| 单批 Probe 数 | 6 |
| 连接 timeout | 10 s |
| 单 Probe wall time | 20 s |
| 单批 wall time | 90 s |
| 单 Probe stderr | 32 KiB |
| 单批 stdout + stderr | 2 MiB |

Probe 自己的上限可以更低。transport 在启动前同时检查 entry budget、剩余批次时间和剩余输出预算；剩余时间成为当前 probe 的更低有效 timeout，剩余时间为零或无法预留该 probe 最大输出时不启动 child。预算不能由 UI、MCP、Connection config 或调用方放宽。

## 8. 错误与输出语义

| 条件 | `ErrorCode` | retryable |
|---|---|---|
| alias/config/ProbeId 无效 | `InvalidDomainValue` | false |
| 本地 `/usr/bin/ssh` 缺失或无法启动 | `Internal` | false |
| Host Key changed / verification failed | `HostKeyMismatch` | false |
| publickey/agent/permission authentication failure | `AuthenticationFailed` | false |
| DNS、route、refused、connect timeout | `NetworkUnreachable` | true |
| probe wall timeout | `ProviderUnavailable` | true |
| stdout/stderr 或批次输出超限 | `InvalidResponse` | false |
| remote command non-zero 且无法安全细分 | `ProviderUnavailable` | false |
| caller cancellation | `Cancelled` | false |

生产分类器只能匹配固定 OpenSSH error signatures，并立即丢弃原始 stderr。公开 message 使用固定文本，不含捕获内容、alias、地址、账户或路径。成功结果只向后续 parser 提供有界 stdout bytes、ProbeId、elapsed 和退出状态；stderr 永不成为 Resource attribute。

Host Key mismatch 在任何阶段都使整个 sync fatal；不能提交先前探针结果，也不能自动 retry、`accept-new` 或把旧主机 Resource 标成 `unhealthy`。普通单 Probe timeout/parser failure 在后续 collector 中可形成 partial，但本 transport 不自行决定 `SyncCoverage`。

## 9. Descriptor

首版 Descriptor 固定：

- `connector_type = "ssh"`、`connector_version = "1.0.0"`、`config_schema_version = 1`。
- `auth.kind = SshAgent`；minimum permission 描述为既有 SSH alias 与非 root 只读 probe 权限，不声明 Keychain Secret。
- 只声明 `Full` 与 `Targeted`；Targeted 只能定位 `ssh.host` 的稳定 external ID，不能携带 command。
- default concurrency 为 1，recommended interval 为 300 秒。
- `ssh.host`、filesystem/process/launchd/systemd 等后续资源能力在 mapper 完成前均为 `Partial`，reason 指向 `CON-G6-02/03/04`。
- known gaps 至少包含无任意命令、无自动 Host Key 接受、不读取 env/history/files/secrets/logs、无 root 默认、无 Windows/非 systemd init 支持、live alias 未验证。

## 10. 自动化验收

必须覆盖：

1. alias 边界值与 `-oProxyCommand`、空白、newline、glob、slash、colon、shell metacharacter 注入拒绝。
2. argv 精确顺序、固定 executable、null stdin、无 shell 和所有安全 `-o`。
3. 公开 API 无 command/string argv setter；未知 ProbeId 在 spawn 前失败。
4. Host Key mismatch、认证失败、网络失败、未知 255、普通 non-zero 的结构化分类与清洗。
5. stdout/stderr 分别超限、timeout、cancel 时精确 child 被终止并 reaped。
6. 单 Probe 与批次预算无法放宽，剩余预算不足时不 spawn。
7. Probe Registry 无重复 ID；六个 entry 的 platform/version/budget 固定；外部 metadata 不暴露 command。
8. Descriptor 通过公共 conformance，且未把 mapper pending module 标记 Supported。
9. `Debug`、Display、serde failure、request summary 和测试快照不含 alias/address/username/path/output sentinel。
10. dependency closure 不含 Store、Sync、Runtime、Query、Tauri、MCP、Keychain、GitHub 或第三方 SSH 协议实现。

验证命令：

```bash
rtk cargo test -p next-infra-connector-ssh
rtk cargo clippy -p next-infra-connector-ssh --all-targets -- -D warnings
rtk cargo fmt --all --check
rtk pnpm --dir apps/desktop run test:dependency-direction
rtk git diff --check
```

不运行 `test:mcp-desktop-smoke`、`security find-identity`、codesign identity、Keychain smoke 或真实 SSH alias。

## 11. 非目标与停止条件

- 不实现 `ReadConnector::sync`、Resource mapper、Normalizer/Store/Sync integration、Runtime registry、UI 或 MCP。
- 不读取真实 SSH config、known_hosts、Agent、IdentityFile 或远程输出，不创建合成 known_hosts 之外的用户状态。
- 不增加 `run_command`、terminal、shell、sudo、root、文件读取、日志读取或端口转发能力。
- 不把 Host Key fingerprint、alias、hostname 或 IP 当作 `external_id`。
- 若实现需要修改共享 Core/Connector API、允许动态 remote command、放宽 Host Key 或预算、解析用户 SSH config，立即停止并回派 Decision/Gate Captain。

## 12. Handoff

Feature Owner 必须报告修改文件、公开 API、固定 argv、预算执行点、错误清洗、测试结果、未执行项和残留风险。Gate Captain 在依赖方向、lockfile 和全 workspace 回归通过前不得派发 `CON-G6-02/03/04`。
