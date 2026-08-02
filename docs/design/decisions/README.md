# Goal 1 决策索引

**状态：** Accepted（Goal 1 本地实现基线）  
**合并日期：** 2026-08-02

本目录冻结进入 Goal 1 前必须明确的工具链、Desktop 生命周期、Bridge 发布边界与 Keychain/签名边界。四项决策共同生效；摘要不能替代各决策正文。

| 决策 | 状态 | 结论摘要 |
| --- | --- | --- |
| [DEC-G1-01](./DEC-G1-01-toolchain-and-crates.md) | Accepted | 精确固定工具链、Tauri 版本、Cargo package、依赖方向和 Rust → TypeScript binding |
| [DEC-G1-02](./DEC-G1-02-desktop-lifecycle.md) | Accepted | 单实例优先、close-to-hide、BackgroundOnly、默认关闭自动登录和 `user_quit` 语义 |
| [DEC-G1-03](./DEC-G1-03-bridge-install-and-upgrade.md) | Accepted for local development | App/Bridge 组成 Release Set；稳定路径、初始协议 `1.0` 与双向区间协商、可信拉起和整体回滚已冻结 |
| [DEC-G1-04](./DEC-G1-04-keychain-signing.md) | Accepted technical boundary | Data Protection Keychain、开发/发布身份隔离、Developer ID 与无在线 updater 边界已冻结 |

## 当前就绪结论

- Git 已初始化，Goal 0 基线已提交；用户已经授权按任务表开始实现。
- 本地 Goal 1 使用 Mock/Fixture，不安装 Agent 集成、不创建 Keychain item，也不修改外部基础设施，因此可以开始。
- Goal 1 ad-hoc Mock/Fixture 构建使用占位 bundle ID `dev.guoxudong.next-infra.dev`；它只是无 Keychain entitlement 的 bootstrap identity，不是 `DEC-G1-04` 定义的 Apple Development identity，也不得用于发布、Keychain 实测或可信 MCP 自动拉起。
- 正式 release bundle ID、Apple Team、Developer ID certificate/profile 与公证凭据仍待用户提供。缺少这些条件只阻塞发布门、真实 Keychain smoke 和可信自动拉起验收，不阻塞本地骨架。
- Goal 4 修改 Codex/Hermes 用户配置前仍需独立授权；Goal 10 外部写操作仍需新的设计与授权。

## 变更规则

实现若需要改变精确版本、crate 依赖方向、启动/退出语义、Bridge 路径或协议窗口、Keychain 身份、签名/更新策略，必须先修改对应决策并完成一次新的独立 Review，不能在功能提交中顺带改变。
