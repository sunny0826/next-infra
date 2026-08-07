# CON-G6-02 通用 Host Probe Parser 与 Mapper 任务冻结

**冻结日期：** 2026-08-06  
**状态：** `FROZEN / READY`  
**依赖：** `CON-G6-01 REVIEW`  
**独占路径：** `crates/next-infra-connector-ssh/src/probes/common.rs`

## 1. 唯一目标

解析 `host.identity.v1`、`host.uptime.v1`、`host.filesystems.v1` 与 `host.process_summary.v1` 的有界 stdout，并映射三个稳定聚合资源及两条 Provider relation。不得修改 transport、Registry command、Descriptor、共享 manifest 或其他 probe 文件。

## 2. 输出合同

| Resource kind | external ID | attributes v1 | health |
|---|---|---|---|
| `ssh.host` | `ssh-host:v1:<host_identity>` | `platform`、`architecture`、`uptime_bucket` | identity+uptime 成功为 Healthy，否则 Unknown |
| `ssh.filesystem` | `ssh-filesystems:v1:<host_identity>` | `entries[]`：`filesystem`、`blocks_kib`、`used_kib`、`available_kib`、`capacity_percent`、`mount` | Unknown |
| `ssh.process-summary` | `ssh-process-summary:v1:<host_identity>` | `total`、`states{}`；丢弃 command/name | Unknown |

两条 `ssh.contains`：Host → Filesystem Summary、Host → Process Summary。evidence key 分别 `ssh-provider-filesystems:<host_identity>`、`ssh-provider-process-summary:<host_identity>`；field path 固定为 `attributes.host_identity`。

`name/display_name` 使用固定产品文本与已配置 alias 的安全展示值；不得读取远程 hostname。scope 由调用方传入。labels 只含 `ssh.platform` 与 `ssh.resource_type` 低基数值。

## 3. Parser 规则与预算

- 所有输入先验证 UTF-8、总字节不超过 Registry 上限、无 NUL/control/private-key/Bearer sentinel；错误和 Debug 不回显输入。
- identity 精确两行：platform 只接受 `Darwin|Linux`，architecture 只接受 1..64 ASCII `[A-Za-z0-9_.-]`。
- uptime 仅解析 `LC_ALL=C uptime` 中 ` up ` 段，映射 `lt_1h | 1h_1d | 1d_7d | 7d_30d | ge_30d | unknown`；不保存 raw uptime、load average、用户数或时间。
- `df -Pk`：必须有 POSIX header；最多 128 entries；数字使用 u64 checked parse；capacity `0..=100%`；filesystem/mount 各最多 256 bytes，拒绝 control/secret sentinel；输入顺序后稳定按 mount/filesystem 排序。
- `ps -Ao state=,comm=`：最多 4,096 rows；只读取首个 state ASCII 字符并按固定 `running/sleeping/stopped/zombie/other` 聚合；command/name 完全丢弃，不进入错误、Debug、Resource 或 label。
- 一个 parser 失败时保留其他成功模块并返回逐 module failure；Host identity 失败为当前 common mapper fatal，因为其他资源无法建立稳定 endpoint。

## 4. 非目标与验收

不实现 collector/ReadConnector、macOS/Linux service、Normalizer registry、UI/MCP 或真实 SSH。测试必须覆盖 Darwin/Linux、uptime formats、df 空格/截断/overflow/128 cap、process 4,096 cap、secret sentinel、稳定排序、stable identity/relation 与 parser partial。验证：SSH crate tests/Clippy、rustfmt、diff check。

