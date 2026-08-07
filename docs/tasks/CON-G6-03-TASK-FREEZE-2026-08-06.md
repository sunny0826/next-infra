# CON-G6-03 macOS launchd Probe Parser 与 Mapper 任务冻结

**冻结日期：** 2026-08-06  
**状态：** `FROZEN / READY`  
**依赖：** `CON-G6-01 REVIEW`  
**独占路径：** `crates/next-infra-connector-ssh/src/probes/macos.rs`

## 1. 唯一目标

解析 `macos.launchd_services.v1` 的 `launchctl list` 有界 stdout，只映射 `SshConnectionConfigV1.allowed_service_ids` 明确允许的 label。不得修改 Registry command、transport、common/Linux probe 或共享 wiring。

## 2. 映射合同

- kind：`ssh.launchd-service`。
- external ID：`ssh-launchd-service:v1:<host_identity>:<service_label>`；service label 已是用户显式 allowlist 与 display name，不再猜测或 hash。
- attributes v1：`service_label`、`loaded`、`pid_present`、`last_exit_status`。不读取 plist、ProgramArguments、env、KeepAlive、日志或文件路径。
- health：`last_exit_status == 0` 且 pid present 为 Healthy；非零为 Degraded；按需/无 PID 且 status 0 为 Unknown。
- relation：Host → Service，kind `ssh.contains`；evidence `ssh-provider-launchd:<host_identity>:<service_label>`，field path `attributes.service_label`。

## 3. Parser 与 partial

- 接受 header `PID Status Label` 与 tab/空格分隔行；最多 2,048 provider rows、最多 64 allowlisted outputs。
- PID 只接受 `-` 或正 u32；status 为有符号 i32；label 必须通过现有 `ServiceId` 语法。
- 未在 allowlist 的行完全丢弃；不得在 warning/error/Debug 中回显。
- allowlist 中未出现的 service 不创建资源，也不推断 stopped/down；module 保持 complete current view，并由后续 collector 通过 missing evidence 规则处理。
- duplicate label、畸形 allowlisted row、超限或 secret sentinel 为 module failure；不影响 common/Linux 成功输出。

## 4. 非目标与验收

不读取真实 Mac mini、launchd plist/log/env，不使用 root/sudo，不实现 collector/UI/MCP。测试覆盖 header、loaded/running/on-demand/nonzero、allowlist filter、missing、duplicate、row/output caps、sentinel redaction、stable output/relation。验证：SSH crate tests/Clippy、rustfmt、diff check。

