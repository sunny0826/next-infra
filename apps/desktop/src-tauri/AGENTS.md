# apps/desktop/src-tauri — Tauri Host（Rust）

## OVERVIEW
Desktop Host 组合根：单一 `next-infra` 二进制，负责生命周期、20 个查询/绑定/GitHub 命令、GitHub 只读同步、本地 RPC 服务与定时同步。

## 结构
```
src/
├── main.rs           # → next_infra_desktop_adapter::run()（薄入口）
├── lib.rs            # TauriBuilder：single_instance/autostart 插件、setup、invoke_handler、退出语义
├── composition/      # ★ 组合根：AppState、命令注册、sync_github、scheduler 接线、purge
├── scheduled_sync.rs # 定时同步驱动（std thread，10s tick，仅 github live 路径）
├── github_live.rs    # GitHub Token 本地文件（0700/0600、O_NOFOLLOW、非 symlink）
├── host/             # lifecycle（LaunchSource）、authorization、local_rpc（Unix socket）、effects（NSWorkspace 睡眠/唤醒）
├── adapter/          # DesktopQueryAdapter + 命令 DTO + LocalSettings/RuntimeCapabilities
└── keychain/         # Keychain 平台封装（MVP 未启用，Token 走文件）
```

## 生命周期关键语义
- `LaunchSource`：UserInteractive / LoginAutostart / McpAuthorized（决定 runtime 模式与是否建窗）。
- close→hide 而非退出；显式 Quit 经托盘/`user_quit` 标记；单实例插件。
- `AppState` 拥有 Runtime（`Arc<Mutex>`）、Store、Query、Scheduler 驱动线程、pending_syncs、GitHub secrets、LocalRpcHost。
- 关闭顺序：显式 Quit / power-off 先停 scheduler 驱动线程 → 停 LocalRpc → `runtime.stop()`（drain writer + checkpoint）。
- 睡眠/唤醒走 NSWorkspace 通知 → `runtime.sleep()/wake()`；wake 的 catch-up 计划入 pending 队列由驱动消费（每连接至多一次）。
- 定时同步：启动与建连时注册（descriptor 推荐间隔，github=900s）；单飞守卫（AtomicBool）防并发，到期被拒则下一间隔自然重试；purge 移除调度条目但驱动线程保持存活（仅 Quit/Power-off 停止）。
- 竞态安全顺序：resolve 连接 → begin（swap 单飞守卫）→ enqueue；purge 全程持守卫。

## 安全边界
- Token 只进 `github_secrets`（0700/0600）；不进入 SQLite/日志/错误/DTO/URL/命令行。
- 命令错误统一 `ErrorEnvelope{code,message,retryable}`，不暴露 store/内部错误原文。
- MCP 自动拉起需授权；显式 Quit 后不自动拉起。
- Provider 只读：无写命令；GitHub 两阶段建连（验证→选仓库）后才同步，全量同步强制 `selected_repository_ids`。

## 测试
- `#[cfg(test)]` 在 composition/mod.rs 与 scheduled_sync.rs：`test_home()` TempDir + 真实 SharedStore + `AppState::open`。
- 每个 open 的测试必须以 `persist_user_quit_and_stop()`（或 `handle_power_off`）收尾，保证驱动线程 join。
- 命令：`rtk cargo test --package next-infra-desktop-adapter`。
