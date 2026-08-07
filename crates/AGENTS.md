# crates/ — Rust workspace 分层

**归属：** 25 个 crate，按层组织；架构依赖边界由 `apps/desktop/scripts/check-cargo-dependencies.mjs` 白名单图强制（Core/Store/Sync/Query/Runtime 不得依赖 Tauri）。

## 层与 crate
| 层 | crates | 说明 |
|----|--------|------|
| 领域 | `next-infra-core` | 无框架依赖的领域类型/ID/端口（约 9 个子模块 flat re-export） |
| 存储 | `next-infra-store` | SQLite WAL 单写者；`projection.rs`（2587 行）为唯一 raw SQL 层 |
| 同步 | `next-infra-sync` | `SyncEngine` 运行生命周期 + `WriterQueue` 单写队列（唯一写边界） |
| 查询 | `next-infra-query` | `QueryService` + 全部 DTO（ts-rs 生成源，`typescript-bindings` feature） |
| 运行时 | `next-infra-runtime` | `Runtime` 状态机（start/sleep/wake/stop）+ `Scheduler` + `CommittedQuerySource` |
| RPC/MCP | `next-infra-local-rpc`、`next-infra-mcp` | Unix Socket 协议 / MCP 只读投影 |
| 绑定/推理/归一 | `next-infra-binding`、`next-infra-inference`、`next-infra-normalizer` | 本地绑定 / 推断证据 / 观察归一化 |
| 宿主 | `next-infra-host-integration` | Desktop Host 与 MCP Bridge 共享的路径/授权/user_quit |
| Connector | `next-infra-connector-*`（12 个） | 见下节 |

## Connector crate 共享契约
- 全部实现 `ReadConnector` + `descriptor()`（ConnectorDescriptor）；**只读**，无写操作。
- 凭据只以 `SecretValue` 短暂传递；输出永不含 secret/响应原文/主机名/IP。
- GitHub/SSH 为大 crate（多子目录、transport+mapper+probe）；Aliyun/Tencent/Supabase* 为单文件 lib.rs，与云模块一一对应（Aliyun 与 Tencent 几乎镜像，一处 bug 大概率另一处也有）。
- GitHub Full 同步强制 `config.selected_repository_ids`（`selected_repository_ids()` 为运行时守卫，缺失即 `InvalidResponse`）；SSH 固定 argv、探针预算、禁止放宽 Host Key 校验。
- 部分 Provider 无 `Incremental` 模式（Aliyun/Tencent/GitHub）；同步失败保留已成功 observation 并标记 Partial。

## 高危文件
| 文件 | 触碰须知 |
|------|---------|
| `next-infra-store/src/projection.rs` | 手写 raw SQL；`purge_connection` 按固定顺序级联删除 10 张表（inference_outputs→relation_versions→changes→bindings→relations→resource_versions→resources→connector_state→sync_runs→connections），破坏顺序触发 FK 错误；改 schema/purge 顺序会损坏既有库。改前先跑 `preview_connection_purge` |
| `next-infra-sync/src/engine.rs` | 公开 API 刻意窄（start/commit/fail/recover）；`fail()` 不推进 cursor；复杂逻辑均为私有 helper；`MissingResource` 入队前原子失败 |
| `next-infra-runtime/src/query_source.rs` | 只读路径；`QueryContextSnapshot` 构造后不可变，refresh 强制 revision 单调递增 |
| `next-infra-connector-github/src/connector.rs` | 垂直有界收集 + route/page cache（ETag）；`Incremental` 返回错误；config 契约：必须含非空 `selected_repository_ids`（`validate_connection_input` 与 `selected_repository_ids()` 双处校验，改契约须两处同步） |
| `next-infra-connector-tencent/src/lib.rs` | TC3-HMAC-SHA256 签名复杂；`tc3_authorization` 的 9 参数为规范所需勿删 |

## 契约与所有权
- `crates/next-infra-core/**`、migration 编号、RPC 协议、DTO/QDTO 均有单写 Owner（见 `docs/tasks/README.md` §3）；非所属 Owner 不得修改共享契约。
- Bridge 二进制不打进 Desktop App bundle（`check-bundle-boundary.mjs` 验证）。
