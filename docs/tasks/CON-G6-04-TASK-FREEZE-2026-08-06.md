# CON-G6-04 Linux systemd Probe Parser 与 Mapper 任务冻结

**冻结日期：** 2026-08-06  
**状态：** `FROZEN / READY`  
**依赖：** `CON-G6-01 REVIEW`  
**独占路径：** `crates/next-infra-connector-ssh/src/probes/linux.rs`

## 1. 唯一目标

解析 `linux.systemd_services.v1` 的 `systemctl list-units --type=service --all --no-pager --no-legend --plain` 有界 stdout，只映射配置 allowlist 中的 unit。不得修改 Registry command、transport、common/macOS probe 或共享 wiring。

## 2. 映射合同

- kind：`ssh.systemd-service`。
- external ID：`ssh-systemd-service:v1:<host_identity>:<unit>`；unit 必须以 `.service` 结尾并通过 `ServiceId` 语法。
- attributes v1：`unit`、`load_state`、`active_state`、`sub_state`；description 丢弃。
- health：`active` 为 Healthy，`failed` 为 Unhealthy，`activating|deactivating|reloading` 为 Degraded，其他为 Unknown；不从 description 猜测。
- relation：Host → Service，kind `ssh.contains`；evidence `ssh-provider-systemd:<host_identity>:<unit>`，field path `attributes.unit`。

## 3. Parser 与 partial

- 每行按前四个 ASCII whitespace fields 解析 UNIT/LOAD/ACTIVE/SUB，剩余 description 忽略；最多 2,048 provider rows、64 allowlisted outputs。
- state 每项 1..32 lowercase ASCII `[a-z-]`；unit 最多 128 bytes。
- 未在 allowlist 的 unit 完全丢弃；不回显 description、未知行或 sentinel。
- allowlist 中未出现的 unit 不创建资源，也不伪装 stopped/down。
- duplicate unit、畸形 allowlisted row、超限或 secret sentinel 为 module failure；未支持 init system 由 collector 标记 Unsupported，不复用 systemd parser 猜测。

## 4. 非目标与验收

不读取 unit file、journal、env、drop-in、root/system mutation，不实现 collector/UI/MCP。测试覆盖 active/failed/transitional/inactive、allowlist、description discard、missing/duplicate、caps、sentinel redaction、stable order/relation。验证：SSH crate tests/Clippy、rustfmt、diff check。

