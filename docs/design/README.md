# Next Infra 设计文档

本目录是 Next Infra 的权威设计基线。Goal 0 已完成，Goal 1 前置决策已冻结并获准开始本地实现；独立 HTML 原型与 Interface System 仍是界面验收规范，不能被脚手架默认样式取代。

## 已确认约束

- 产品采用单实例、自托管、本机运行方式。
- 仅面向当前操作系统用户，不设计多用户、团队、租户、RBAC 或远程协作。
- Rust 负责独立于 UI 框架的 Control Plane Runtime、领域模型、连接器、同步引擎、存储、本地协议和 MCP Bridge。
- 前端使用 React/TypeScript，并由 Tauri v2 Desktop Host 提供桌面窗口、托盘和系统集成。
- 第一阶段只查看外部基础设施，不修改外部资源。
- Hermes 与 Codex 通过 MCP 调用同一查询服务。
- 不把 Next Infra 设计成 Terraform、监控平台、日志平台或密码管理器的替代品。

## 当前关键决策

| 决策 | 结论 |
| --- | --- |
| 运行模型 | 一个单实例 Tauri Desktop Host 常驻托盘，内部承载 Rust Control Plane Runtime |
| 本地存储 | SQLite；WAL、单写入者、有限历史、默认不保存原始响应 |
| Desktop UI | React/TypeScript SPA，通过受限 Tauri Commands 调用 Query Service，Events 仅通知失效 |
| Agent 接入 | 独立 `next-infra-mcp` 通过 STDIO 接入客户端，再通过 Unix Domain Socket 查询 Desktop Host |
| 外部连接 | 连接器通过厂商 API 或系统 OpenSSH 读取资源 |
| 凭据 | 数据库只保存 `SecretRef`，真实秘密进入 macOS Keychain |
| 扩展方式 | 首版为编译期 Rust connector；运行时插件延后 |
| 写操作 | 不进入首版；以后使用独立的 Plan、Approval、Execute、Verify 契约 |

## 文档索引

1. [本地环境基线](./00-local-environment.md)：记录当前主机能力和由此产生的设计约束。
2. [RFC-0001：单机架构](./RFC-0001-single-node-architecture.md)：项目定位、整体架构、进程模型和主要替代方案。
3. [结构图](./architecture-diagrams.md)：系统上下文、进程所有权、Runtime 组件、读写路径和 Host 生命周期。
4. [术语表](./glossary.md)：跨文档、未来 Schema、UI 和代码使用的规范名称与消歧规则。
5. [资源与存储模型](./resource-and-storage-model.md)：资源身份、版本、关系、同步覆盖和 SQLite 投影。
6. [连接器与同步契约](./connector-and-sync-contract.md)：连接器边界、同步事务、限流、删除语义和覆盖路线。
7. [Agent 接口与安全](./agent-interface-and-security.md)：MCP 工具、响应边界、本地通信和凭据策略。
8. [界面与可视化设计](./visualization-and-interaction.md)：信息架构、视觉语义、拓扑边界和桌面交互状态。
9. [串行 Goal 与并行任务](./implementation-goals.md)：定义不可跨越的 Goal 验收顺序。
10. [Goal 1 决策索引](./decisions/README.md)：工具链、生命周期、Bridge、Keychain/签名的冻结状态与当前阻塞。
11. [Goal 1 独立 Review](./decisions/REVIEW-G1-2026-08-02.md)：记录工程入口首轮 blocker、修复和 fresh re-review。
12. [文档 Review 报告](./REVIEW-2026-08-02.md)：记录 Tauri 架构调整后的审查发现、修复和保留风险。
13. [HTML 交互原型](../../prototype/README.md)：用虚构 Fixture 验证 Overview、Topology、Evidence Spine 和窄屏布局。
14. [Interface System](../../.interface-design/system.md)：固化已通过浏览器验收的 UI token、响应式框架、重复组件和状态语义，是 React 实现的视觉规范。
15. [Luna Worker 并行任务总表](../tasks/README.md)：按 Runtime、Connector、Desktop UI 拆解依赖波次、独占路径、验收和 Gate Captain。
16. [任务拆解独立 Review](../tasks/REVIEW-2026-08-02.md)：记录首轮 blocker、修订和二轮 `PASS`。

## 文档治理

- 主架构结论以 RFC-0001 为准。
- 组件、进程和调用方向以《结构图》作为 RFC 的可视化投影。
- 跨文档命名以《术语表》为准。
- 领域数据语义以《资源与存储模型》为准。
- Connector 行为以《连接器与同步契约》为准。
- MCP、安全和秘密处理以《Agent 接口与安全》为准。
- 如果文档冲突，应先修改主 RFC 并记录取舍，再同步下游文档。
- Goal 验收门保持串行；Goal 内并行规则、任务 ID 和文件所有权以 `docs/tasks/` 为准。
- 开发已经获准，但只能按通过 Gate 的任务包修改其独占路径；共享 manifest、lockfile、生成 DTO 和 entrypoint 仍由对应 Captain 串行维护。

## 当前非目标

- 多用户登录、团队空间、RBAC、审计合规平台。
- 通过公网访问 Next Infra 控制平面。
- 首版提供浏览器版 UI、loopback HTTP API 或远程 MCP。
- 收集和长期保存完整日志、指标或 GitHub Actions 工件。
- 自动推断并合并所有跨平台资源身份。
- 让 Agent 执行任意 SSH 命令。
- 首版覆盖阿里云、腾讯云的所有产品。
- 首版修改、重启、部署或删除任何外部基础设施。
