# PROJECT KNOWLEDGE BASE — next-infra

对于可快速完成、边界明确、可重复的小型任务使用 deepseek_worker 来完成。

**Generated:** 2026-08-07 · **Commit:** c2e9a5e · **Branch:** main

## OVERVIEW
单实例、单 macOS 用户、本机 Tauri Desktop Host 的基础设施观测控制平面：Rust workspace（Control Plane + SQLite 单写者）+ React/TypeScript（Vite/vitest）。所有对外 Provider 始终只读。分层 AGENTS：`crates/`、`apps/desktop/src/`、`apps/desktop/src-tauri/`、`docs/`。

## STRUCTURE
```
next-infra/
├── apps/
│   ├── desktop/           # Tauri v2 App：src/（React 前端）+ src-tauri/（Rust Host）+ e2e/
│   └── mcp-bridge/        # MCP STDIO 桥接二进制（薄壳，逻辑在 crates/next-infra-mcp）
├── crates/                # 25 个 workspace crate，分层 + 强制依赖边界（见 crates/AGENTS.md）
├── tests/integration/     # 3 个顶层集成测试 crate
├── fixtures/connectors/   # replay JSON（仅 fixture-* 合成数据）
├── docs/                  # 架构/RFC/决策/运维/任务验收（见 docs/AGENTS.md）
└── prototype/             # 探索性原型，非 workspace 成员
```

## WHERE TO LOOK
| 任务 | 位置 |
|------|------|
| Tauri 命令 / AppState / 生命周期 / 定时同步驱动 | `apps/desktop/src-tauri/src/composition/mod.rs`、`scheduled_sync.rs` |
| 前端页面 / 桌面适配器 / i18n / 测试 fixture | `apps/desktop/src/**` |
| 领域类型与端口（Connection/SyncTrigger/StoreReader/…） | `crates/next-infra-core` |
| SQLite 读写 / 迁移 / 连接 purge | `crates/next-infra-store`（projection.rs 为高危大文件） |
| 同步运行生命周期 / 单写队列 | `crates/next-infra-sync` |
| 查询服务 / DTO（ts-rs 生成源） | `crates/next-infra-query` |
| 运行时状态机 / Scheduler / 恢复 | `crates/next-infra-runtime` |
| 12 个 Connector crate（provider 全只读） | `crates/next-infra-connector-*` |
| 本地 RPC / MCP 只读投影 | `crates/next-infra-local-rpc`、`crates/next-infra-mcp` |
| 路径/授权/user_quit/睡眠唤醒 | `crates/next-infra-host-integration` + `apps/desktop/src-tauri/src/host/` |
| 产品目标 / 运维边界 / 交接状态 | `docs/design/implementation-goals.md`、`docs/OPERATIONS.md`、`docs/HANDOFF-*.md` |

## CODE MAP
| Symbol | 位置 | 角色 |
|--------|------|------|
| `next_infra_desktop_adapter::run()` | `apps/desktop/src-tauri/src/main.rs` | Desktop Host 入口 → TauriBuilder |
| `AppState` | `composition/mod.rs` | 中央状态：Runtime/Store/Query/Scheduler/GitHub secrets/本地 RPC/关闭生命周期 |
| `composition::invoke_handler()` | `composition/mod.rs` | 全部 20 个 Tauri 命令注册 |
| `sync_github` / `enqueue_github_sync` | `composition/mod.rs` | GitHub 只读同步（单飞守卫） |
| `ScheduledSyncDriver` | `scheduled_sync.rs` | 定时同步驱动线程（仅 github 有 live 路径） |
| `NextInfraMcp` / `serve_stdio()` | `crates/next-infra-mcp` | MCP 只读投影（7 tools + 2 resources） |
| `Runtime<B,Q>` / `Scheduler` | `crates/next-infra-runtime` | 生命周期状态机 + 到期调度 |
| `SyncEngine<S>` / `WriterQueue<S>` | `crates/next-infra-sync` | 唯一写边界（单写队列） |
| `QueryService` / `ErrorEnvelope` | `crates/next-infra-query` | 只读查询 + 统一错误信封（code/retryable） |

## CONVENTIONS（非标准项）
- 所有 shell 命令经 `rtk` 前缀：`rtk cargo test --workspace`、`rtk pnpm --dir apps/desktop test`；cargo 一律 `--locked`。
- Node 必须 `>=24.12.0 <25`（`.node-version`=24.12.0）；当前 v26 会打 engine warning，发布/验收用 Node 24。
- Rust→TS 类型生成：`cargo test -p next-infra-query --features typescript-bindings --test export_types` → `apps/desktop/src/generated/query/`（52 DTO）。
- 依赖边界由 `scripts/check-cargo-dependencies.mjs` 强制（28 包白名单；仅 `next-infra-desktop-adapter` 可依赖 Tauri）。
- 错误信封统一 `{schema_version, code, message, retryable}`；SQLite 单写者，Connector/Normalizer 不直写 DB。
- 前端默认 zh-CN；组件测试用 `MockDesktopAdapter` + fixture（`fixture-*` 前缀、`example.test`、固定时间戳）。
- `docs/tasks/README.md` §3 定义单写所有权表：修改共享契约前先读。

## ANTI-PATTERNS（本项目禁止）
- Token/Secret/真实 Provider 响应/主机名/IP/仓库名**永不**进入 Git、Fixture、日志、错误、文档、URL、命令行、DTO。明文 Secret 仅存 SQLite `connection_secrets`（DB 0600/目录 0700、FK 级联清理、无投影读取）；Token 仍**永不**进入 Git/Fixture/日志/错误/文档/URL/命令行/DTO。
- **不**手工编辑 SQLite / Application Support / Token 文件清理数据。
- **不**实现 Provider 写操作/部署/重启/外部删除/任意 SSH 命令（Goal 10 仅设计）。
- local replay / Fixture / Browser-Vite 测试**不得**冒充真实 Provider、真实 Tauri Host 或 MCP live 验收（live 需真实凭据/真实 Agent/签名身份/原生 App smoke）。
- 生成文件（`apps/desktop/src/generated/**`、src-tauri/gen、lockfile）**禁止手改**。
- 不得使用 `[patch.crates-io]` / Git revision / 未记录的 registry 替换；CI/验收必须 `--locked`/`--frozen-lockfile`。
- 非所属 Owner 不得修改共享契约（core/store/migration/rpc/DTO）；同一任务只允许一个活跃 owner。
- SSH：不得加 `StrictHostKeyChecking=no|accept-new` 或 `UserKnownHostsFile=/dev/null`，不读 env/history/secrets。

## COMMANDS
```bash
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
rtk pnpm --dir apps/desktop lint          # tsc --noEmit
rtk pnpm --dir apps/desktop test          # 依赖方向检查 + vitest（85 测试）
rtk pnpm --dir apps/desktop build
rtk pnpm --dir apps/desktop tauri dev
rtk pnpm --dir apps/desktop tauri build   # → target/release/bundle/
# DTO 变更后重新生成 TS 绑定：
cd crates/next-infra-query && rtk cargo test --features typescript-bindings --test export_types
```

## NOTES
- 2026-08-07 已修复：`next-infra-connector-github` 的 config 契约统一为必须含非空 `selected_repository_ids`（`validate_connection_input` 原要求空对象，与范围选择策略矛盾，导致建连 validate 必失败 + 4 个预存测试失败；fixture 同步更新）。
- 交接文档 `docs/HANDOFF-2026-08-07.md` 与 `docs/OPERATIONS.md` 是安全/运维边界权威来源；P0/P1/P2 优先级见交接文档。
- `AGENTS.md` 首行是 worker 路由偏好（未提交变更，不属于产品代码基线）。
