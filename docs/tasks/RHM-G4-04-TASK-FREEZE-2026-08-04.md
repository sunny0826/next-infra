# RHM-G4-04 Trusted Host Availability 与 user_quit Task Freeze

**日期：** 2026-08-04  
**状态：** `READY / LIVE-SIGNATURE-SMOKE-BLOCKED-ENVIRONMENT`  
**独占路径：** `apps/mcp-bridge/src/availability/**`、Bridge library composition、Desktop `user_quit` read/clear module；共享 entrypoints 在各子模块 Review 后串行接入。

## 1. 固定顺序

Bridge 每个进程只执行一次以下状态机：

```text
connect existing UDS
  ├─ success -> start MCP STDIO
  └─ unavailable
       -> inspect user_quit
       -> validate integration record and release paths
       -> require allow_mcp_auto_launch=true
       -> verify App + Bridge signatures/requirements
       -> fixed /usr/bin/open argv once
       -> bounded wait <= 10s
       -> require Local RPC handshake success
```

任何步骤失败都返回 `host_unavailable`；不得循环拉起、读取 SQLite 兜底或接受 Agent 提供的 executable/path/argv。

## 2. `user_quit` fail-closed

路径固定：

```text
~/Library/Application Support/Next Infra/state/user-quit-v1.json
```

- marker 不存在：允许继续检查 integration record。
- marker 存在且 schema 1 / `user_quit=true`：抑制。
- marker 存在但 symlink、错误 owner/mode/type、损坏、未知 schema、字段缺失、`user_quit=false` 或 unreadable：全部抑制。
- Bridge、MCP request、upgrade 和 background launch 永远不能删除或改写 marker。
- Desktop MCP launch 在启动 Runtime 前再次读取 marker；存在/损坏/unreadable 时拒绝启动。
- 只有 interactive launch 或已启用的 login launch 可以原子删除 marker；第二实例、wake、crash recovery、upgrade 与 MCP launch不能清除。

State directory 必须是当前 euid 的真实 `0700` directory，marker 必须是当前 euid 的普通 `0600` file。

## 3. Integration Record v1

严格 serde model 使用 `deny_unknown_fields`，字段与 `DEC-G1-03` 完全一致。验证至少包括：

- schema 精确为 1；协议 major/minor/window 与编译时 contract 相容；
- capability sets 确定性、无重复、无未知项，Host/Bridge required/supported 与当前 Bridge contract 一致；
- release ID 与当前 installed Bridge release 一致；
- stable App 精确为 `~/Applications/Next Infra.app`；
- stable Bridge 精确为 `.../integration/mcp/current/next-infra-mcp`；
- record 为 euid-owned `0600` regular file，所有固定 parent 为 euid-owned `0700` real directories；
- `current` 是相对 symlink，只能指向 `releases/<single-release-id>`，无 `..`，解析后 executable 等于当前进程 artifact；
- App/Bridge path、bundle ID、Team 和 designated requirements 不接受 argv、env、Provider 或 MCP input 覆盖；
- `allow_mcp_auto_launch` 必须显式为 true；ad-hoc/unsigned verifier 永远不能产生 authorized 结果。

## 4. Production signature verifier

macOS production verifier 固定调用系统二进制，不经 shell：

```text
/usr/bin/codesign --verify --strict --verbose=4 -R=<recorded requirement> <artifact>
```

并独立读取/核对 App bundle identifier、Team identifier 和 designated requirement。所有 argv 都来自已验证 record；空 requirement、缺 Team 的 auto-launch record、codesign 非零、输出解析歧义或 artifact path replacement 均 fail closed。

Verifier trait 允许 tests 使用 fake signed artifacts，但 fake 结果必须明确标为 fixture，不能写成 live signature passed。

当前 `security find-identity -p codesigning -v` 为 `0 valid identities found`，因此 Developer ID live smoke 保持环境阻塞。

## 5. 固定 launcher

唯一命令：

```text
/usr/bin/open -g <validated stable_app_path> --args --background --launch-source=mcp
```

- `Command` 直接 argv，不使用 shell/PATH。
- launcher trait 的 fake 记录精确 argv 和调用次数；每 Bridge process 最多一次。
- open 成功不等于 Host ready；只以同一 UDS 的 peer UID + protocol handshake 成功为 ready。
- wait 使用 bounded monotonic deadline、短退避，最多 10 秒；`user_quit` 在等待期间出现时立即停止。

## 6. Desktop Host 二次授权

- MCP launch 参数只选择 `BackgroundOnly`，不构成授权。
- Desktop 必须读取同一 marker 和 integration record；只有 `allow_mcp_auto_launch=true` 且本地 record 校验通过才允许 `LaunchSource::McpAuthorized`。
- MCP launch 不创建 WebView、不清除 marker；Runtime ready 后开始 UDS accept。
- Interactive/login 清除 marker 必须在读取并确认 launch source 后发生；清除失败则 fail closed，不继续启动。

## 7. 验收

必须覆盖：

1. running Host 直接连接，不读取/写入 integration state，不调用 launcher。
2. valid user_quit、损坏 marker、symlink、wrong mode、unreadable 全部抑制当前和新 Bridge。
3. missing/invalid/unknown integration record、auto-launch false、path/capability/release mismatch 全部 fail closed。
4. unsigned/ad-hoc、wrong Team/bundle/requirement 全部不 launch。
5. valid fake-signed record 只调用一次固定 `/usr/bin/open` argv。
6. bounded wait 最多 10 秒；成功必须完成真实 Local RPC handshake。
7. wait 期间 user_quit 出现立即停止，不能复活 Host。
8. Desktop interactive/login 可清除 marker；MCP/second-instance/wake/upgrade 不能。
9. 多进程测试证明一个 Bridge launch attempt，用户 Quit 后当前和新 Bridge均抑制。
10. Developer ID live smoke 单独报告，不以 fake/ad-hoc 替代。

## 8. 非目标与授权边界

本任务不安装 App/Bridge、不生成 integration record、不修改 Codex/Hermes 配置、不签名/公证/发布，也不执行任何 Provider 或基础设施写操作。
