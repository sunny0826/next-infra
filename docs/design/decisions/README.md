# 设计决策索引

**状态：** Accepted（Goal 1 本地实现基线）  
**合并日期：** 2026-08-02

本目录冻结进入 Goal 1 前必须明确的工具链、Desktop 生命周期、Bridge 发布边界与 Keychain/签名边界。四项决策共同生效；摘要不能替代各决策正文。

| 决策 | 状态 | 结论摘要 |
| --- | --- | --- |
| [DEC-G1-01](./DEC-G1-01-toolchain-and-crates.md) | Accepted | 精确固定工具链、Tauri 版本、Cargo package、依赖方向和 Rust → TypeScript binding |
| [DEC-G1-02](./DEC-G1-02-desktop-lifecycle.md) | Accepted | 单实例优先、close-to-hide、BackgroundOnly、默认关闭自动登录和 `user_quit` 语义 |
| [DEC-G1-03](./DEC-G1-03-bridge-install-and-upgrade.md) | Accepted for local development | App/Bridge 组成 Release Set；稳定路径、初始协议 `1.0` 与双向区间协商、可信拉起和整体回滚已冻结 |
| [DEC-G1-04](./DEC-G1-04-keychain-signing.md) | Accepted technical boundary | Data Protection Keychain、开发/发布身份隔离、Developer ID 与无在线 updater 边界已冻结 |
| [DEC-G3-01](./DEC-G3-01-committed-query-source.md) | Proposed for Review | Runtime-owned SharedStore、DTO-neutral Store projection、CommittedQuerySource 与单 SQLite owner |
| [DEC-G4-01](./DEC-G4-01-local-rpc-v1.md) | Accepted for Goal 4 implementation | 4-byte BE framed JSON、双向版本/能力协商、七个只读 Query variant 与严格资源上限 |
| [DEC-G5-01](./DEC-G5-01-github-live-path.md) | Accepted for local-file GitHub MVP | GitHub connection UI 以本地 `0700`/`0600` 文件保存 token；validation/full sync 只读且不录制响应 |
| [DEC-G6-01](./DEC-G6-01-ssh-identity-and-probe-budget.md) | Accepted for Goal 6 implementation | opaque UUID host identity、严格 Host Key、固定 OpenSSH argv、六个版本化 Probe 与硬预算 |
| [DEC-G7-01](./DEC-G7-01-binding-inference-topology-timeline.md) | Accepted for Goal 7 implementation | Binding lifecycle、deterministic inference provenance、bounded topology/timeline 与 schema v2 upgrade |
| [DEC-G8-01](./DEC-G8-01-dokploy-database-scope.md) | Accepted for Goal 8 implementation | Dokploy Database 在本 Goal 明确 unsupported；仅实现五类无 secret 摘要资源 |
| [DEC-G9-01](./DEC-G9-01-provider-boundaries.md) | Accepted for Goal 9 implementation | Supabase managed/self-hosted 分离；Aliyun/Tencent 按产品 module 独立覆盖 |
| [Goal 10 Operation RFC](../operation-capability-rfc.md) | Proposed for independent review | 写能力仅定义 Plan/Diff/Approval/Execute/Verify，不实现外部操作 |

## 当前就绪结论

- Git 已初始化，Goal 0 基线已提交；用户已经授权按任务表开始实现。
- 本地 Goal 1 使用 Mock/Fixture，不安装 Agent 集成、不创建 Keychain item，也不修改外部基础设施，因此可以开始。
- Goal 1 ad-hoc Mock/Fixture 构建使用占位 bundle ID `dev.guoxudong.next-infra.dev`；它只是无 Keychain entitlement 的 bootstrap identity，不是 `DEC-G1-04` 定义的 Apple Development identity，也不得用于发布、Keychain 实测或可信 MCP 自动拉起。
- 正式 release bundle ID、Apple Team、Developer ID certificate/profile 与公证凭据仍待用户提供。缺少这些条件只阻塞发布门、真实 Keychain smoke 和可信自动拉起验收，不阻塞本地骨架。
- Goal 4 修改 Codex/Hermes 用户配置前仍需独立授权；Goal 10 外部写操作仍需新的设计与授权。
- Goal 6 的内部 OpenSSH transport/fixture 实现可开始；真实 SSH alias、MCP 和 Apple signing identity 均不是当前前置条件，也不得用 fixture 冒充 live 验收。

## 变更规则

实现若需要改变精确版本、crate 依赖方向、启动/退出语义、Bridge 路径或协议窗口、Keychain 身份、签名/更新策略，必须先修改对应决策并完成一次新的独立 Review，不能在功能提交中顺带改变。
