# DEC-G1-03：MCP Bridge 安装、可信拉起与升级策略

**状态：** Accepted for local development；发布签名条件沿用 `DEC-G1-04`  
**决策日期：** 2026-08-02  
**适用范围：** 单用户 macOS 的 `Next Infra.app`、`next-infra-mcp` 与本地 Agent 集成  
**不构成：** 安装、升级、Codex/Hermes 配置变更或工程实现授权

## 1. 唯一决策

Next Infra 将 Desktop App 与 MCP Bridge 作为同一个 **Release Set** 的两个独立产物发布：

- Desktop App 的稳定路径固定为 `~/Applications/Next Infra.app`。
- Bridge 不进入 App Bundle、sidecar、Desktop Cargo package 或 `Contents/MacOS`。
- Bridge 安装到当前用户 Application Support 的版本目录，Agent 始终调用一个原子切换的稳定路径。
- 安装器一次性 stage、验证并切换同一个 Release Set；失败时整体回滚到上一组 App + Bridge。
- Host/Bridge 先做 protocol handshake；只接受同 major 且相邻一个 minor 的恢复窗口，能力不满足时明确拒绝。
- MCP 自动拉起默认关闭，只能由 Desktop UI 交互式授权；Agent 参数、Provider 内容和 Bridge 自身都不能开启授权。
- `user_quit` 存在、损坏或不可读时一律 fail closed，Bridge 不拉起 Host。

Goal 1 只创建可独立构建的 Bridge target，不安装它、不切换 Release Set、不修改 Agent 配置。真实安装与 Agent acceptance 属于 Goal 4。

## 2. 路径与权限

逻辑路径固定为：

```text
~/Applications/Next Infra.app

~/Library/Application Support/Next Infra/
  integration/
    mcp/
      releases/
        <release_id>/
          next-infra-mcp
          release-manifest-v1.json
      current -> releases/<release_id>
      integration-v1.json
  state/
    user-quit-v1.json
  run/
    next-infra-v1.sock
```

规则：

1. `integration/`、`mcp/`、`releases/`、`state/` 和 `run/` 必须由当前 UID 拥有且为 `0700`。
2. Bridge executable 必须是普通文件、不可为 symlink、由当前 UID 拥有，且 group/other 不可写。
3. 只有 `current` 可以是 symlink；它必须是相对链接，目标只能是同一 `releases/` 下的单层 `release_id`，不得包含 `..`。
4. Agent 配置的 command 固定为 `.../integration/mcp/current/next-infra-mcp`，不能指向版本目录、App Bundle 或任意 PATH 命令。
5. App 稳定路径的父目录与 bundle owner 必须是当前 UID；解析后的 bundle 路径必须精确等于安装记录，不跟随外部 symlink。
6. 所有 staging、切换和回滚都在各自目标文件系统的同一父目录完成，以便使用同卷原子 `rename(2)`。

`release_id` 是构建生成的不可变值，例如 `<semver>+<build-id>`；目录一旦发布不得原地修改。

## 3. Release Manifest 与可信安装记录

### 3.1 Release Manifest

每个 Bridge 版本目录包含只读 `release-manifest-v1.json`：

```text
schema_version
release_id
host_version
bridge_version
protocol_major
protocol_minor
minimum_supported_minor
host_supported_capabilities
host_required_capabilities
bridge_supported_capabilities
bridge_required_capabilities
bridge_sha256
expected_host_bundle_id
expected_team_id | null
```

Manifest 是一致性输入，不是签名替代品。Developer ID 发布必须同时验证 App、Bridge 的 code signature、Team 和 designated requirement；本地 ad-hoc build 只能用于 Mock/Fixture，不能启用可信自动拉起。

### 3.2 Integration Record

`integration-v1.json` 由 Desktop 的受限本地配置服务原子写入，权限为 `0600`：

```text
schema_version
release_id
stable_app_path
stable_bridge_path
bundle_id
team_id | null
app_designated_requirement
bridge_designated_requirement
protocol_major
protocol_minor
minimum_supported_minor
host_supported_capabilities
host_required_capabilities
bridge_supported_capabilities
bridge_required_capabilities
allow_mcp_auto_launch
installed_at
updated_at
```

- Agent、Bridge 参数和 Provider response 都不能写此文件。
- 未知 schema、字段缺失、owner/mode 错、路径不匹配或 JSON 损坏时，Bridge 返回 `host_unavailable`。
- 本地未签名/adhoc 开发记录必须强制 `allow_mcp_auto_launch=false`。
- 正式记录的 Team、bundle ID 与 requirements 来自已验证 artifact，不接受 UI 文本输入。

同用户恶意进程不在完整防御范围内，但 owner、mode、路径、hash 和 code-signing requirement 检查可以防止误启动、损坏安装和参数注入。

## 4. Protocol Handshake

首个 Local RPC 协议版本固定为：

```text
protocol_major = 1
protocol_minor = 0
minimum_supported_minor = 0
```

这些常量在 Goal 1 仅作为冻结契约；握手与 Query capability 到 Goal 4 实现。后续每个 Release Set 的兼容窗口最多覆盖当前与前一 minor，即 `protocol_minor - minimum_supported_minor <= 1`。

Local RPC 握手在任何 Query 之前完成：

```text
client: protocol_major, protocol_minor, minimum_supported_minor,
        bridge_version, release_id, supported_capabilities, required_capabilities
host:   protocol_major, protocol_minor, minimum_supported_minor,
        selected_protocol_minor, host_version, release_id,
        supported_capabilities, required_capabilities
```

兼容规则：

1. major 必须相同，否则返回 `protocol_mismatch`，不得发送 Query。
2. 令 `lower = max(client.minimum_supported_minor, host.minimum_supported_minor)`，`upper = min(client.protocol_minor, host.protocol_minor)`；只有 `lower <= upper` 才兼容，协商值固定为最高交集 `selected_protocol_minor = upper`。这同一算法同时覆盖 N/N、N/N-1 和 N-1/N。
3. capability 必须双向检查：`client.required_capabilities` 是 `host.supported_capabilities` 的子集，且 `host.required_capabilities` 是 `client.supported_capabilities` 的子集；任一不满足均拒绝，不能猜测字段或静默降级权限。
4. 初始 Host `1.0` 支持 `query.search_resources.v1`、`query.get_resource.v1`、`query.get_topology.v1`、`query.get_health_summary.v1`、`query.get_recent_changes.v1`、`query.get_sync_status.v1`、`query.list_connector_coverage.v1`，且不要求 Bridge capability；初始 Bridge `1.0` 要求这七项且不声明额外 supported capability。
5. Host 与 Bridge release ID 不同但版本区间和 capability 均兼容时允许只读查询，并返回 `upgrade_recommended=true`。
6. 超出窗口时返回双方版本、支持范围和无秘密修复指引；Bridge 不读取 SQLite 兜底。
7. protocol compatibility tests 必须保存为 golden fixtures，并覆盖 1.0/1.0、未来 N/N、N/N-1、N-1/N、无区间交集、major mismatch 和双向 capability mismatch。

相邻 minor 窗口只用于升级/回滚短期恢复，不承诺长期兼容；每个 Release Set 仍必须以相同 release ID 完整安装。

## 5. 自动拉起与 `user_quit`

Bridge 的顺序固定为：

1. 尝试连接已知 UDS，并验证父目录、Socket owner/mode 和 peer UID。
2. Socket 不存在时读取 `user-quit-v1.json`；存在、损坏或不可读均返回 `host_unavailable`，不继续。
3. 读取并验证 `integration-v1.json`；`allow_mcp_auto_launch` 不是 `true` 时返回 `host_unavailable`。
4. 验证 App stable path、owner、bundle ID、Team、designated requirement 和当前 Release Set。
5. 只用固定 `/usr/bin/open` argv 后台打开已验证的 App，参数固定为 `--background --launch-source=mcp`；不经 shell，不接受 MCP/Agent 提供路径或额外参数。
6. 使用有界退避等待同一个 UDS 最多 10 秒；只以 Socket handshake 成功作为 ready。
7. 超时、进程退出或协议不兼容时返回结构化错误；一次 Bridge 进程最多尝试启动一次，禁止循环复活。

启动参数只用于展示模式分类，不是授权。Host 必须再次读取 `user_quit` 和可信本地配置；MCP 路径永远不能清除 marker。用户显式 Quit 后，当前 Bridge 和新 Bridge 都不得自动拉起 Host。

## 6. 安装、升级与回滚事务

### 6.1 首次安装

1. 在 `releases/.staging-<release_id>` 写 Bridge + Manifest，fsync 文件和目录。
2. 在 `~/Applications/.Next Infra.<release_id>.staged.app` stage App。
3. 校验 owner、普通文件/symlink 边界、hash、bundle ID、版本、签名、entitlements 和 protocol range。
4. 将 Bridge staging 原子 rename 为不可变版本目录。
5. 将 staged App 原子 rename 为稳定 App 路径。
6. 创建临时 `current` symlink 并原子 rename 到正式 `current`。
7. 最后原子写入 Integration Record；之前任一步失败都不得留下“已安装”记录。
8. Agent 配置必须由用户显式执行或授权单独的集成步骤，不能由 App 静默修改。

### 6.2 升级

1. 完整 stage 并验证新 Release Set，不修改 current。
2. 请求旧 Host 完成 `DEC-G1-02` 的 shutdown；未能安全停止则中止切换。
3. 将旧 App rename 为同目录 rollback path，再把新 App rename 到稳定路径。
4. 原子切换 `current` 到新 Bridge；随后原子更新 Integration Record。
5. 启动新 Host并完成 N/N handshake、Query smoke 和 bundle boundary check。
6. 失败时停止新 Host，恢复旧 App、旧 `current` 与旧 record，再执行 N-1/N-1 smoke。
7. 新版本通过后至少保留一个上一 Release Set；更旧版本按磁盘预算清理。

切换不是跨文件系统的单个原子操作，因此“原子升级”定义为：任一时刻 Integration Record 只指向一组已完整验证的 Release Set，并具有可验证的事务日志和整体回滚；绝不留下永久不兼容的混合终态。

### 6.3 卸载

- 先禁用集成并删除 Integration Record，再移除 stable Bridge link 和 release directories。
- 是否删除 App、SQLite、logs 和 Keychain items 是独立用户选择；默认保留本地数据与 Secret。
- 不静默编辑 Codex/Hermes 配置；显示精确移除指引并允许用户确认。

## 7. 失败恢复

| 失败点 | 恢复结果 |
| --- | --- |
| staging/hash/signature 失败 | current 与 App 不变；删除 staging |
| 新 App 已切换、Bridge 未切换 | 事务日志驱动恢复旧 App；record 仍指旧 release |
| Bridge 已切换、record 未更新 | 旧 Host/新 Bridge 只在相邻 minor 窗口内工作；恢复任务优先回滚 current |
| record 已更新、Host smoke 失败 | 整体回滚 App/current/record |
| crash 留下 transaction journal | 下次用户交互式启动或 installer 恢复；Bridge 自动拉起返回 unavailable，不自行修复 |
| rollback artifact 缺失/校验失败 | 禁用 auto-launch并返回 repair-required；不猜测可执行文件 |
| user_quit 已 latched | 安装/升级不得清除；新版本仍保持抑制 |

事务日志只能保存 release/path/hash/state，不含 Secret、Agent 输入或 Provider 数据；权限 `0600`。

## 8. Goal 1 / Goal 4 验证合同

Goal 1 只验证构建与 bundle 边界：

```bash
rtk cargo build -p next-infra-mcp-bridge --bin next-infra-mcp --locked
rtk pnpm --dir apps/desktop tauri build
rtk test pnpm --dir apps/desktop test:bundle-boundary
rtk proxy test ! -e "<Next Infra.app>/Contents/MacOS/next-infra-mcp"
rtk proxy test ! -e "<Next Infra.app>/Contents/Resources/next-infra-mcp"
```

Goal 4 在测试专用 Application Support 和 `~/Applications` staging 根目录执行：

```bash
rtk test cargo test -p next-infra-local-rpc protocol_compatibility
rtk test cargo test -p next-infra-mcp host_availability
rtk proxy codesign --verify --strict --verbose=4 "<Next Infra.app>"
rtk proxy codesign --verify --strict --verbose=4 "<next-infra-mcp>"
rtk proxy stat -f '%Su %Sp %N' "<integration-root>" "<stable-bridge>" "<Next Infra.app>"
rtk proxy readlink "<integration-root>/mcp/current"
rtk proxy codex mcp add --help
```

真实 Codex acceptance 必须单独获得修改用户配置的授权、记录原配置和恢复步骤，再通过 stable Bridge path 完成七个只读工具查询。Hermes 命令在安装后按当时版本重新确认，不能提前写死。

## 9. 非目标与变更触发条件

非目标：不实现 installer/updater，不修改用户 Agent 配置，不读取 SQLite/Keychain，不启用在线 updater，不安装 LaunchDaemon/helper，不支持 system-wide、多用户、远程 MCP 或 App Store。

以下变化必须重开本决策：

- App/Bridge 改为同 bundle、sidecar、独立安装包或 system-wide 路径。
- protocol window、Release Set manifest、签名 requirement 或 bundle ID/Team 改变。
- 引入在线 updater、自动回滚服务、helper/XPC 或 Runtime 独立进程。
- Codex/Hermes 不再支持稳定 STDIO command path。
- macOS 对 `~/Applications`、Developer ID、LaunchServices 或 symlink/rename 行为产生不兼容变化。

## 10. 用户选择与当前阻塞

- MCP 自动拉起的产品默认值已经固定为关闭；未来用户只能从 Desktop UI 显式开启。
- 本地 Goal 1 不需要选择 release bundle ID、Team 或公证凭据，使用 Mock/Fixture 且不安装集成。
- Developer ID 发布与真实 Keychain/auto-launch acceptance 仍受 `DEC-G1-04` 的外部身份条件阻塞，不能用 ad-hoc 结果替代。
- Codex/Hermes 用户配置变更需在 Goal 4 单独授权；当前不得执行。

## 11. 官方依据

- [Tauri v2 macOS App Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)
- [Tauri v2 macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri v2 Sidecar](https://v2.tauri.app/develop/sidecar/)（本文明确不采用）
- [Apple TN3127：Inside Code Signing Requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)
- [Apple Developer ID](https://developer.apple.com/developer-id/)
- 本机 `man 2 rename`、`man 1 open` 与 `codex mcp add --help`；具体命令在对应 Goal 执行时重新验证。
