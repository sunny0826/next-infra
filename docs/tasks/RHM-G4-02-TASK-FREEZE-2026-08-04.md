# RHM-G4-02 Secure Unix Socket RPC Task Freeze

**日期：** 2026-08-04  
**状态：** `READY`  
**独占实现路径：** `crates/next-infra-local-rpc/src/transport/**`、`crates/next-infra-local-rpc/src/session/**`、对应 tests；冻结的 `src/protocol/**` 只能由 G4-01 follow-up 串行修改。

## 1. 已解除的协议阻塞

Socket session 必须在任何 Query 前完成 handshake。G4-01 follow-up 已增加专用 `HandshakeResponse`：

```text
accepted: host hello + selected minor + upgrade_recommended
rejected: structured RpcError, no request_id
```

该阻塞已解除；G4-02 必须消费冻结格式，不得自行定义第三种格式。

## 2. 固定平台与 I/O 模型

- 首版目标仅为已冻结的 macOS 单用户 Desktop Host。
- 使用 `std::os::unix::net::{UnixListener, UnixStream}` 和一个受控 server thread；不引入 Tokio、HTTP、TCP 或远程 listener。
- 每个 frame 先读取精确 4-byte header，检查 `1 MiB` 上限后才分配 body。
- Client/Server 都复用 `protocol` codec 和 typed envelopes，不复制 JSON contract。

## 3. 路径、owner 与 mode

测试使用显式临时根；生产 composition 后续传入：

```text
<app-support>/run/next-infra-v1.sock
<app-support>/run/next-infra-v1.lock
```

- `run/` 必须是当前 effective UID 拥有的真实目录，不是 symlink，mode 精确为 `0700`。
- lock 必须是 owner-only 普通文件，不是 symlink，mode `0600`。
- socket bind 后 mode 立即设为 `0600`；accept/connect 前后均验证 owner/type/mode。
- 任何 symlink、错误 owner、group/other permission 或非预期 file type 都 fail closed，不清理、不覆盖。

## 4. 单 Server 与 stale socket

- Server 启动先以 `O_NOFOLLOW` 打开 lock file，并持有 non-blocking exclusive `flock` 直到 shutdown/drop。
- 无法取得 lock 表示可能有 live owner，返回 already-running/host-unavailable，不删除 socket。
- 只有持有 exclusive lock、确认 socket owner/type/mode 正确、且 connect 明确返回 refused/not-found 时，才允许删除 stale socket。
- 活跃 socket、无法判定错误、错误 owner/mode、symlink 或普通文件均不得删除。
- 正常 shutdown 只删除本实例创建且 identity 仍匹配的 socket；不得删除后来替换的路径。

`flock` 是首版 process-liveness proof：live Host 持锁；进程崩溃时内核释放锁。PID 文件或 `kill(pid, 0)` 不作为单独证据，避免 PID reuse。

## 5. Peer UID 与 session

- Server 对每个 accepted stream 使用 macOS `getpeereid`；peer UID 必须等于当前 effective UID。
- Client 连接后同样验证 server peer UID。
- 任何 Query 前必须完成 `ClientHello` → `HandshakeResponse`。
- Rejected handshake 后关闭 session；Accepted 后只接收 frozen `RequestEnvelope`。
- request/response 仍使用相同 request ID；未知 frame/variant 返回安全错误并终止或按可恢复边界处理。

## 6. 并发与 Query adapter

- 每个 session 最多 8 个 in-flight Query；第九个必须返回 `too_many_requests`，不得进入无界 queue。
- response writer 必须串行，允许不同 request ID 的 response 按完成顺序返回。
- Query handler 只适配七个 `QueryService` 入口；Query error 清洗后映射为 `query_failed`。
- 不允许 Store、Keychain、Connector、Tauri 或 Provider 依赖进入 protocol/transport。

## 7. 验收证据

必须覆盖：

1. parent `0700`、lock/socket `0600`。
2. symlink、普通文件、错误 mode 和错误 owner fixture fail closed（错误 owner 在平台能力允许的隔离 fixture 中验证；不可安全构造时记录环境阻塞，不伪造）。
3. 第二 server 不能取得 lock，也不能删除 active socket。
4. crash-like drop 释放 lock；只有拒绝连接的正确 socket 可被清理。
5. peer UID 检查成功；可注入 peer verifier 覆盖拒绝分支。
6. fragmented header/body、oversized header、EOF、invalid JSON。
7. Query 前发送 request 被拒绝。
8. protocol/capability mismatch 返回专用 rejected handshake。
9. 七个 Query 只通过 typed handler；第九个 in-flight 返回 `too_many_requests`。
10. shutdown 后 socket 清理，非本实例替换路径不被删除。

## 8. 非目标

不实现 MCP STDIO、Host 自动拉起、`user_quit`、Bridge 安装、Codex/Hermes 配置、Tauri composition、真实 Provider/Secret 或任何外部写操作。
