# docs/ — 项目知识库导航

## OVERVIEW
架构 / RFC / 设计决策 / 运维边界 / 任务验收文档。`HANDOFF-*.md` 与 `OPERATIONS.md` 是权威入口。

## 权威文档
| 文档 | 内容 |
|------|------|
| `HANDOFF-*.md` | 当前交接状态、P0/P1/P2 优先级、禁止事项、验证基线 |
| `OPERATIONS.md` | 运维与凭据边界、SQLite/Token 处理规则 |
| `design/implementation-goals.md` | Goal 0-10 产品目标 |
| `design/RFC-0001-single-node-architecture.md` | 架构权威（单写者、只读 Provider） |
| `design/decisions/DEC-*.md` | 已冻结设计决策（G1 工具链/生命周期、G4 RPC、G5 GitHub、G6 SSH、G8 范围、G9 Provider） |
| `design/glossary.md` | 术语表 |

## tasks/ 命名约定（导航关键）
- 前缀含义：`DEC-G*` 设计决策、`RHM-G*` 实现任务、`CON-G*` Connector/契约、`UI-G*` 前端、`GATE-G*` Goal 验收门、`*-TASK-FREEZE` 契约冻结。
- `tasks/README.md` §3 是**单写所有权表**（root manifest/lockfile、core、migration、DTO、Tauri 配置、RPC 协议、Shell 路由各归唯一 Owner）——改代码前先读。
- §4 派发协议（Task ID/Status/Objective/…）与 §7 Gate Captain 职责：任务包必须含验收与验证命令。
- `GATE-*` 记录每 Goal 通过状态；`COMPLETION-AUDIT-*` 汇总完成性证据与外部缺口。

## 规则
- 决策必须落到冻结文档才算数；冲突以冻结文档为准。
- 禁止把真实 Token/主机/IP/Provider 响应写入任何文档。
- 术语一致性：优先用 `design/glossary.md` 的定义（Connection/Resource/SyncRun/Evidence…）。
