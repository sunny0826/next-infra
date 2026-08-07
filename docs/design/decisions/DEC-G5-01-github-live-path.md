# DEC-G5-01: GitHub 本地只读连接 MVP

- **状态：** Accepted for local-file GitHub MVP
- **日期：** 2026-08-07
- **适用范围：** GitHub 连接创建、凭据本地保存、只读验证/同步与桌面展示
- **不构成：** 外部写操作、Keychain、OAuth/GitHub App、MCP 实测、签名/公证或其他 Provider 的连接流程

## 1. 决策

首个真实数据路径是 GitHub。用户在 Desktop 的 Connectors 页面输入 fine-grained PAT 后，Desktop Host 先以 `/user` 做只读验证，并以单页、最多 100 个结果的 `/user/repos` 列出可访问仓库。用户必须明确勾选至少一个仓库；验证通过后创建本地 connection，保存 token，并立即执行一次仅覆盖这些仓库的 `Full` 只读同步。后续从同页发起的 Manual Sync 保持相同范围，不会默认同步整个 GitHub 账户。

MVP 不改变 Core 的 `SecretRef` 或 SQLite 合同：`Connection.secret_ref` 保持 `None`，token 只按 `ConnectionId` 保存在应用数据目录的私有文件中。Keychain 与 Apple identity 是低优先级的后续强化项，不阻塞本机单用户路径。

## 2. 凭据边界

| 事项 | 约束 |
| --- | --- |
| 输入 | React 密码输入框仅在提交期间保留 token，通过一次 React → Tauri IPC 调用交给 Desktop Host，随后在 `finally` 清空 |
| Token 形状 | Fine-grained PAT；仅选择需要展示的仓库；最小权限为 `Metadata: read`、`Actions: read`、`Deployments: read` |
| 禁止权限 | 不请求 `Contents`、`Administration`、`Secrets`、`Variables` 或任意 write 权限 |
| 保存 | `<Application Support>/github-secrets-v1/<connection-id>.token`；目录必须为当前用户的 `0700`，文件必须为当前用户拥有的 `0600` 常规文件 |
| 文件安全 | 拒绝符号链接和宽松权限；写入使用临时文件、同步并原子替换；读取与创建使用 `O_NOFOLLOW` |
| 禁止持久化 | token 不进入 SQLite、设置、日志、错误、fixture、URL、环境变量、命令行或文档 |
| 可读取方 | 仅 Desktop Host 在验证/同步时将文件读取为临时 `SecretValue`；MCP Bridge 与普通 CLI 不读取 token |

本地明文文件是受单实例、本机 macOS 用户边界约束的明确 MVP 取舍，不等同于 Keychain。正式分发前必须重新评估并迁移凭据保存策略。

## 3. 同步与失败语义

- `validate` 使用 GitHub `/user`；响应体不落盘、不进入错误或报告。
- `sync` 只调用既有 GET-only transport，并只对 `Connection.config.selected_repository_ids` 中的仓库按 Repository → Environment → Deployment → Workflow → Run → Job 的既有预算与部分覆盖语义执行。
- 结果经既有 Normalizer 和 SyncEngine 写入 SQLite。SQLite 只保留 allowlisted 观察摘要与关系，不保存 token、原始 response、日志或 artifact。
- 完整同步更新 Connection Health 为 `healthy`；部分覆盖为 `degraded`。认证/凭据失败映射为 `auth_failed`，限流映射为 `rate_limited`，网络或 Provider 不可达映射为 `unreachable`。
- 失败、部分覆盖或凭据文件不可用时，保留已有资源，不创建 tombstone，也不回退到任何非受控的凭据来源。
- 每次只允许一个 GitHub sync 执行；并发请求返回可重试的 `sync_in_progress`。
- 用户可在 Desktop 中预览并确认删除单个 GitHub connection 的本地快照。清理会删除该 connection 的资源、关联关系、版本、变更、失效绑定与同步记录，并移除其本地 token 文件；它绝不向 GitHub 发起写请求，也不影响其他 connection。

## 4. 不录制与验收

用户通过 UI 提供 token 即授权该次只读验证与同步。该路径不创建、修改、部署、重启或删除 GitHub 资源。

真实响应不得写入 fixture、日志、错误、文档或验收记录。任何人工报告只可包含结构化错误码、health、coverage 和计数；不得包含 token、owner/repository 名、路径、URL、IP 或 payload。

MCP、Codex/Hermes、SSH、其他 Provider、Keychain 与签名 smoke 继续保持低优先级和独立验收边界。
