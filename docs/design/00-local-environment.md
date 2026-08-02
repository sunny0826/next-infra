# 本地环境基线

**检测日期：** 2026-08-02  
**工作目录：** `/Users/guoxudong/codes/next-infra`  
**检测方式：** 只读命令；未安装或修改任何系统级工具与服务

## 1. 环境摘要

| 类别 | 检测结果 | 设计影响 |
| --- | --- | --- |
| 操作系统 | macOS 26.5.2，arm64 | 首个运行环境以 macOS 用户级服务为目标 |
| 硬件 | Apple M4，10 核，16 GB 内存 | 足够并发执行少量网络采集与本地可视化 |
| 磁盘 | 数据卷约 460 GiB，仅约 23 GiB 可用，使用率 95% | 必须限制快照历史和日志；不引入必需的容器与 PostgreSQL 服务 |
| Rust | rustc/cargo 1.92.0，stable arm64；Clippy、rustfmt 可用 | Rust 开发工具链完整，可在脚手架阶段固定版本 |
| Node | Node 24.12.0，Corepack 0.34.5 | 满足 React/TypeScript 开发需求 |
| 包管理器 | pnpm 11.9.0，npm 11.6.2 | 前端建议使用 Corepack 并固定 pnpm 版本 |
| Docker | Docker 28.5.2、Compose 5.3.1 已安装，但 daemon 未运行 | Docker 可用于以后测试，但不能成为本机运行前提 |
| SQLite | sqlite3 3.50.6；JSON 函数可用；系统构建未启用 FTS5 | 首版搜索不依赖系统 FTS5；应用需自带能力明确的 SQLite 构建或使用普通索引 |
| PostgreSQL | psql 18.3 客户端可用 | 不代表本地服务可用；单实例设计不采用 PostgreSQL |
| SSH | OpenSSH 10.2p1 | SSH Connector 优先复用系统 OpenSSH 与用户现有 SSH 配置 |
| 凭据存储 | macOS `security`/Keychain 可用；FileVault 已开启 | 秘密进入 Keychain，SQLite 只保留引用；本地数据受系统磁盘加密保护 |
| 进程托管 | launchd/launchctl 可用 | Tauri Autostart 可使用用户级 LaunchAgent；不需要独立 daemon 服务定义 |
| 原生构建 | Xcode 与 Apple Clang 可用 | Tauri、Rust 原生依赖和 macOS App Bundle 的构建前置条件完整 |
| Git/GitHub | Git 2.53.0、GitHub CLI 2.96.0 | 后续可执行版本控制和 GitHub Connector 本地验收 |
| Codex | codex-cli 0.146.0-alpha.9.2；支持 STDIO 与 Streamable HTTP MCP | 可以在本机验证 STDIO MCP |
| Hermes | 未安装 | 本轮不安装；Hermes 实际兼容性验收必须作为后续独立步骤 |
| Tailscale | 未安装 | 不假定远程主机天然可达；SSH 网络连通性由用户现有网络负责 |
| 仓库状态 | Git 已初始化为 `main`，设计基线与 Goal 1 决策已提交；工程脚手架尚未创建 | 可以按已冻结边界进入 Goal 1 |

检测输出中可能出现的设备序列号、硬件 UUID 等标识不写入本项目文档。

## 2. 由环境产生的架构结论

### 2.1 使用 SQLite，不使用 PostgreSQL

单用户、单实例不存在横向扩展、跨节点锁、租户隔离和高写入并发需求。SQLite 可以减少一个长期运行数据库服务，也避免依赖 Docker daemon。同步过程采用并行网络读取、单写入者事务，适合 SQLite 的并发模型。

首版不得依赖宿主机 SQLite 的可选编译能力。具体规则：

- JSON 编解码首先在 Rust 层完成。
- 搜索先使用规范化列、普通索引和有界模糊查询。
- 如果以后使用 FTS5，应用构建必须显式启用并在启动自检中验证。
- 数据库启用 WAL、foreign keys、busy timeout 和定期 checkpoint。

### 2.2 不要求 Docker 常驻

生产形态是本机原生进程。Docker 只可用于可选的集成测试、模拟服务或发布验证，不能成为启动 Next Infra 的必要条件。

### 2.3 使用 Tauri Desktop Host、LaunchAgent 和 Keychain

- Tauri Desktop Host 以当前用户身份运行，不申请 root 权限。
- 应用关闭主窗口后继续驻留托盘；只有显式退出才停止 Control Plane Runtime。
- 自动登录启动由 Tauri Autostart 以用户级 LaunchAgent 实现，业务调度仍由 Control Plane Runtime 管理。
- API Token、PAT、云 Access Key 等秘密通过受限 Tauri Command 或标准输入进入 Keychain。
- 不允许在 CLI 参数、shell rc、SQLite、日志或导出文件中保存明文秘密。

Tauri 只是桌面宿主：领域、同步、存储和 Query Service 必须保留为不依赖 Tauri 的 Rust crate，以便独立测试，并为未来可选 headless host 保留边界。

### 2.4 SSH 复用系统 OpenSSH

复用系统 OpenSSH 可以继承 `~/.ssh/config`、SSH Agent、ProxyJump、硬件密钥和 Host Key 验证。Rust Connector 只执行版本化的固定探针，不向 Agent 暴露任意命令入口。

### 2.5 严格控制本地存储增长

当前磁盘使用率较高，因此默认策略为：

- 不保存外部 API 原始响应。
- 资源未发生语义变化时，只更新 `last_seen_at`，不写入新版本。
- 完整资源版本默认保留 30 天。
- 结构化 Change 默认保留 180 天。
- SyncRun 和普通应用日志默认保留 30 天。
- SQLite 文件采用 1 GiB 软预算，并在达到 70% 与 90% 时告警和收缩历史。
- GitHub Actions 日志、部署日志和监控时序数据只按需读取，不进入长期存储。

这些是可调整的默认值，不是数据库硬上限。

## 3. Goal 1 脚手架固定基线

[`DEC-G1-01`](./decisions/DEC-G1-01-toolchain-and-crates.md) 已固定以下版本文件与依赖规则，由 `RHM-G1-01` 创建：

- `rust-toolchain.toml`：固定 Rust `1.92.0`，附带 rustfmt 与 Clippy。
- `.node-version`：固定 Node.js `24.12.0`。
- 根 `package.json#packageManager`：固定 `pnpm@11.9.0`，通过 Corepack 使用。
- Rust edition：2024。
- Desktop：Tauri Rust `2.11.5`、`tauri-build 2.6.3`、JavaScript API `2.11.1`、CLI `2.11.4` + React/TypeScript。
- SQLite：由 Rust 依赖提供可复现构建，不依赖 `/usr/bin/sqlite3` 的可选模块。

精确插件、crate/package 布局、锁定规则和 QDTO binding 见该决策；普通功能任务不得顺带升级。

## 4. 已执行的代表性验证

```bash
rtk proxy sw_vers
rtk proxy uname -m
rtk proxy rustc --version
rtk proxy cargo --version
rtk proxy cargo clippy --version
rtk proxy cargo fmt --version
rtk proxy node --version
rtk proxy corepack --version
rtk proxy pnpm --version
rtk proxy docker --version
rtk proxy docker compose version
rtk proxy docker info --format '{{.ServerVersion}}|{{.OSType}}|{{.Architecture}}'
rtk proxy sqlite3 --version
rtk proxy psql --version
rtk proxy ssh -V
rtk proxy fdesetup status
rtk proxy codex mcp add --help
rtk proxy hermes --version
rtk proxy tailscale version
rtk proxy git rev-parse --show-toplevel
```

SQLite JSON 与 FTS5 分别使用最小内存数据库进行了能力探测：JSON 成功，FTS5 返回 `no such module: fts5`。

## 5. 环境阻塞项

- Hermes 尚未安装，因此只能完成协议级设计，不能声明 Hermes 端到端验收通过。
- 远程 Mac mini、云主机的 SSH 地址、Host Key 和网络路径尚未提供，不能验证 SSH Connector 可达性。
- 各厂商只读凭据尚未配置，不能验证 API 权限覆盖与速率限制。
- 正式 release bundle ID、Apple Team、Developer ID certificate/profile 与公证凭据尚未提供；本地 Mock/Fixture 开发不受阻，发布与真实 Keychain smoke 保持阻塞。
