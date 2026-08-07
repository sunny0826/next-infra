# CON-G6-05 SSH ReadConnector、Partial 与 Replay 纵切任务冻结

**冻结日期：** 2026-08-06  
**状态：** `FROZEN / READY AFTER CON-G6-02..04 REVIEW`  
**依赖：** `CON-G6-02..04`、`UI-G6-01`  
**独占路径：** SSH connector wiring、Goal 6 synthetic integration/security tests、Goal 6 acceptance records

## 1. 唯一目标

把已冻结的 OpenSSH transport 和四类 mapper 汇合为只读 `ReadConnector`，证明 Host Key fatal、逐 probe partial、SQLite/SyncEngine replay 与 SSH UI fixture 语义。不得修改六个 v1 command，不得加入任意 command/argv/path 输入。

## 2. Connector 流程

1. 本地拒绝错误 connector type、schema、config、cursor 和 targeted locator；不 spawn。
2. 第一批只执行 `host.identity.v1`。无有效 identity、Host Key mismatch、认证、网络、取消均为 sync fatal，不提交 observation。
3. 按 identity 平台执行 common 三个 probe，加且只加一个 platform service probe。不得在 Darwin 上执行 systemd，或在 Linux 上执行 launchd。
4. transport success 交给对应 parser；单 probe transport/parser failure 只使该 module partial，已成功资源保留。Host Key mismatch 在任一批出现均丢弃全部 staged observation。
5. 资源和关系稳定排序并拒绝重复。Debug、warning、failure、summary 与 fixture 不含 alias、地址、用户名、fingerprint、command、description 或 raw output。

`validate` 使用同一固定 identity probe 验证配置与连接，不需要 `SecretValue`，也不读取 Keychain。`sync` 只接受 `Full|Targeted`；incremental 与 cursor 拒绝。

## 3. Coverage 与 missing evidence

- 全部计划 probe 成功且 parser complete 的 Full sync 返回 `Complete + AuthoritativeFull`；allowlist 中本次未出现的 service 不创建 observation，由 SyncEngine missing evidence 规则处理。
- 任一非 identity module 失败返回 `Partial + SyncCoverage::Partial`，不增加 missing count，不触发 tombstone。
- Targeted 输入必须恰好是当前 config 的 `ssh.host` locator。由于现有 `SyncCoverage::Targeted` 只接受 normalization 后的内部 `ResourceId`，connector 当前返回显式 Partial，且不贡献 missing evidence；不得伪造内部 ID。
- 网络不可达、认证失败、Host Key mismatch 与取消不写 observation，因此只由 runtime 更新 Connector Health/Freshness；既有 Resource health 保持最后已知值。

## 4. Error 与 summary

- 保留 `DEC-G6-01` error code/retryable，不包裹或降级 Host Key mismatch。
- parser failure 固定为清洗后的 `InvalidResponse`；warning 只含固定 module ID。
- request summary 只统计 probe request 数、总 elapsed 与固定 `success|failure` 分类；不含 alias 或 remote status 文本。
- 没有任何有效 observation 时返回 fatal；不得用空 partial batch 冒充成功。

## 5. 验收

- fake transport 覆盖 Darwin/Linux 计划、targeted 校验、identity fatal、child partial、Host Key late fatal、cancel、稳定排序和无 command DTO。
- SQLite + SyncEngine 两轮 replay：complete missing 可累计；partial/unreachable 不累计且不误删。
- SSH synthetic QDTO/UI fixture 无真实 alias/IP/output；unreachable 只显示 stale/unreachable，Host 不显示 Unhealthy；Host Key mismatch 明确可见且无接受按钮。
- 验证 SSH/workspace tests、strict Clippy、dependency direction、bindings、desktop tests/build、diff check。

## 6. 明确延后

本任务不测试 MCP、真实 SSH host、Apple Development/Developer ID identity、Keychain、codesign、notarization 或发布。真实 alias smoke 仍为外部验收，不得被 synthetic fixture 代替。
