# Next Infra 操作手册

## 1. 适用范围

Next Infra 是面向单个 macOS 用户、本机运行的只读基础设施视图。Desktop Host、SQLite 数据库和 MCP Bridge 都只在本机用户边界内工作；它不是网页服务，也不会开放 HTTP 端口。

本手册描述当前可交付版本的实际操作边界：启动桌面端、浏览已持久化的资源和关系、检查连接器覆盖范围、管理本地启动偏好，以及开发验证。它不假定已有真实 Provider、SSH 或 MCP 配置。

## 2. 当前能力与限制

| 能力 | 当前状态 | 操作说明 |
| --- | --- | --- |
| 资源浏览、详情、拓扑、时间线 | 可用 | 只显示已经写入本地 SQLite 的经过清洗的观察结果。 |
| Connector Coverage Matrix | 可用 | 显示各 connector 声明的模块覆盖，不表示实时权限或同步成功。 |
| 本地自动登录启动 | 可用 | 在 Settings 中显式开关；默认关闭。 |
| GitHub 新建连接与手动同步 | 可用 | 输入 fine-grained token、加载最多 100 个可访问仓库并明确勾选同步范围；只支持 GitHub。 |
| 其他 Provider Secret 与 SSH | 未开放的用户流程 | 不要把 Token、AccessKey、SSH 私钥写入配置文件、Fixture 或命令行。 |
| 真实 GitHub 同步 | 用户授权后可用 | 通过桌面 UI 录入 token 后执行；其余 Provider 与 SSH 仍未验收。 |
| Codex / Hermes MCP 实际调用 | 延后 | 已有本地 STDIO/Unix Socket 实现，但用户级 Agent 配置和实际查询未验收。 |
| 修改、部署、重启或删除外部资源 | 不支持 | 当前产品始终为只读。 |

`supported`、`partial` 和 `unsupported` 是 connector 的声明覆盖；它们与 Connection Health、某一次 Sync Coverage 和 Resource Health 是四个不同概念。

## 3. 环境要求

- macOS，当前首版使用 Tauri Desktop Host。
- Rust `1.92.0`（`rust-toolchain.toml` 会固定该版本）。
- Node.js `24.12.x` 与 pnpm `11.9.0`。项目会拒绝不在 `>=24.12.0 <25` 范围内的 Node 版本。
- 已安装 Xcode Command Line Tools 和本机 Tauri 所需的 macOS 构建工具。

检查版本：

```bash
rustc --version
node --version
pnpm --version
```

## 4. 本地启动

在仓库根目录准备前端依赖：

```bash
cd /Users/guoxudong/codes/next-infra/apps/desktop
pnpm install --frozen-lockfile
```

启动桌面开发模式：

```bash
pnpm tauri dev
```

Tauri 会启动前端开发服务器和 `next-infra` Desktop Host。主窗口关闭时默认隐藏到托盘，并不退出 Runtime；使用托盘菜单中的 Quit 才是显式退出。

首次或没有连接数据时，页面可以为空。这表示本地库没有可展示的观察记录，不代表任何外部资源被删除或不可用。

## 5. 日常查看流程

### Overview

先查看 Health、Freshness、连接器状态和近期 Change。Resource Health 只反映某条资源观察的状态；Connection Health 和 Freshness 用于判断该数据是否仍值得信任。

### Inventory 与 Resource Detail

使用 Inventory 的筛选和分页定位资源，选择资源打开详情。详情页显示资源属性、观察时间和证据；敏感值、原始 Provider 响应、完整日志、Secret 和 SSH 私钥不应出现。

### Topology

从 Inventory 或全局搜索选中一个资源后进入 Topology。图以焦点资源为中心，并有深度和节点上限；它不是全局无边界关系图。检查边的 evidence 类型：Provider、configured、inferred 三者的可信度不同。

### Timeline

Timeline 只展示已提交的结构化变更。重复且无变化的同步不会生成新的 Timeline 项。利用 SyncRun 与 evidence 信息回溯数据来源，不要将时间线事件理解为外部操作记录。

### Connectors

在 Connectors 页面分别检查：

1. Connection Health 与最近一次同步错误。
2. 最近成功/尝试时间和下一次计划时间。
3. Connector Coverage Matrix 中的模块范围与已知缺口。
4. 对不再需要的 GitHub 连接，选择“删除本地数据”。应用会先展示受影响的资源、关系、版本、变更、绑定与同步记录数量；再次确认后才会删除该连接的本地快照和对应 Token 文件。此操作不可恢复，也不会清理其他连接的数据。

使用 Add GitHub connection 输入连接名称和 fine-grained token，然后选择“验证并加载仓库”。应用只显示最多 100 个可访问仓库；必须至少勾选一个仓库后才能创建连接。首次同步和后续 Manual Sync 都只读取这些已选仓库的基础数据、Actions 与 Deployments，不会默认同步整个 GitHub 账户。其他 connector 不支持此操作。

### Settings

Settings 仅用于本地生命周期、保留策略、数据预算和能力开关。登录自动启动需要用户明确开启；MCP 自动拉起与登录自动启动是独立授权，且当前 MCP 集成尚未验收。

## 6. 凭据与连接安全

不要手工编辑 SQLite 或 Application Support 目录。GitHub MVP 将 token 以明文 BLOB 存储于 SQLite `connection_secrets` 表，适合单实例本机使用：

- Desktop Host 将 token 存储于 SQLite `connection_secrets` 表（`connection_id` → BLOB）；DB 文件必须为当前用户拥有的 `0600`，所在目录必须为当前用户的 `0700`；FK 级联清除（删除 connection 时自动清理 secret 行）。
- token 不写入设置、日志、错误、fixture、URL 或命令行。React 在提交结束后清空密码输入；MCP Bridge、QueryService 与普通 CLI 不能通过投影读取 `connection_secrets` 表。
- `credential_unavailable` 表示 secret 缺失或无法读取，不要通过手工写入 SQLite 来规避。
- **Keychain 方向已取消（2026-08-07 用户决策）**；Secret 一律存 SQLite `connection_secrets`，不追求 Keychain 迁移。
- SSH 使用现有 SSH config alias、SSH Agent 或 IdentityFile；Next Infra 不复制私钥，也没有任意命令入口。
- MCP 工具只读，且不接受 Secret、SSH 命令或任意目标地址作为参数。

## 7. MCP 状态

`next-infra-mcp` 是独立的 STDIO Bridge，通过 Unix Domain Socket 查询本机 Desktop Host。它不直接读取 SQLite、Keychain 或 connector。

当前不要自行向 Codex 或 Hermes 写入 MCP 配置。真实 Agent 查询、可信安装路径、签名 App 自动拉起和 Hermes 安装均处于延后验收状态。已显式退出 Desktop Host 时，Bridge 必须返回 `host_unavailable`，不会自行清除 `user_quit` 标记或重新拉起 App。

## 8. 常见问题

| 现象 | 含义与处理 |
| --- | --- |
| 主窗口关闭后仍有托盘图标 | 正常。选择托盘 Quit 才会停止 Host。 |
| Connector 显示 `partial` | 某些模块、权限、区域、分页或限流信息不完整；已有成功观察会保留，不能据此推断资源已删除。 |
| `credential_unavailable` | GitHub token 文件缺失、非当前用户拥有或权限不是 `0600`。不要将 Secret 写入日志或配置。 |
| Manual Sync 不可用 | 只有 enabled 的 GitHub 连接支持手动同步；其他 connector 尚未开放。 |
| 需要移除一次范围过大的 GitHub 同步 | 在 Connectors 的对应 GitHub 行选择“删除本地数据”，核对预览数量并再次确认；不要手动编辑 SQLite。 |
| MCP 返回 `host_unavailable` | 先交互式启动 App；若用户曾显式 Quit，保持该状态。不要通过重启 Bridge 绕过它。 |
| 页面没有资源 | 本地 SQLite 没有已写入观察，或当前过滤条件不匹配。它不是外部资产盘点结论。 |

## 9. 开发验证与故障定位

从仓库根目录运行 Rust 回归：

```bash
cd /Users/guoxudong/codes/next-infra
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

从 Desktop 目录运行前端验证：

```bash
cd /Users/guoxudong/codes/next-infra/apps/desktop
pnpm test
pnpm run build
```

构建失败时，先确认 Node 与 Rust 版本；再检查是否已有另一个 Tauri 开发实例占用同一单实例 Runtime。不要清空 SQLite 或 Application Support 目录作为排障的第一步，因为这会移除本地观察历史或凭据。

## 10. 发布前与真实环境验收

在新的明确授权下，按以下顺序恢复外部验收：

1. 配置最小权限、只读的 Provider 身份，逐 connector 验证真实账户、区域、分页和限流。
2. 用已有 SSH alias 对固定 probe 做一次真实 read-only 验收，确认 Host Key 不匹配会失败。
3. 在 Codex 与 Hermes 各执行一次只读 MCP 查询，确认输出与 Desktop Query Service 一致。
4. 提供 Apple Development 或 Developer ID identity 后，再验证可信 MCP 自动拉起、签名、公证和 macOS 交互生命周期。

在以上步骤完成前，项目状态应保持“本地只读首版完成，外部验收待授权”。权威状态记录见 [完成性审计](./tasks/COMPLETION-AUDIT-2026-08-06.md)。
