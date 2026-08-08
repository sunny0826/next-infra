# RFC: 操作能力控制面

**作者:** Maintainer  
**状态:** Proposed（待独立评审）  
**日期:** 2026-08-06

## 1. 背景

Goal 0-9 建立了本地单实例、自托管、只读的 Infra 观察系统。后续若加入
资源变更，必须先固定控制面契约，避免把只读凭据、MCP 查询工具或任意 SSH
命令直接升级为写权限。

本文只定义设计，不授权执行，不创建 Action Connector，不修改云资源、用户级
MCP 配置或凭据。

## 2. 问题陈述

基础设施写操作具有不可逆、异步和权限扩散风险。单纯使用一个 `approved`
布尔值无法表达审批范围、计划变化、执行状态、回读验证和失败原因；而将写
能力附加到 `ReadConnector` 会使最小权限边界失效。

## 3. 提议的解决方案

### 3.1 能力与凭据隔离

`ReadConnector` 与 `ActionConnector` 是两个不兼容的 trait，并使用不同的
凭据引用类型。`SecretRef` 不能在内存或配置中升级为 `ActionSecretRef`。
每个动作必须声明 provider、资源范围、风险等级和输入 schema。Action secret
不得进入 React、Query DTO、只读 MCP、日志或持久化计划。

### 3.2 生命周期

```text
Draft -> Planned -> AwaitingApproval -> Approved -> Executing
                                             |            |
                                             v            v
                                          Rejected     Verifying
                                                          |
                                     +--------------------+----------------+
                                     v                                     v
                                  Succeeded                            Failed
```

每次状态转移记录 actor、时间、计划 fingerprint、原因和前一状态。审批不是
布尔值，而是包含作用域、过期时间、风险确认和明确 action ID 列表的结构化
决定。计划 fingerprint 变化或审批过期后必须重新审批。

### 3.3 Plan、Diff、Execute、Verify

- `Plan` 是不可变、规范化、有界的动作集合，拥有确定性 fingerprint。
- `Diff` 只包含变更前后摘要和脱敏未知项，不包含响应原文或秘密。
- `Execute` 只接受已批准的 fingerprint；每个 action 使用幂等键，并在 Provider
  调用前后写入追加式审计事件。
- `Verify` 必须通过现有只读 Connector 重新读取；HTTP 2xx 本身不代表成功。
- 仅允许 Provider 明确声明安全且幂等的动作重试，并受 deadline 限制。

### 3.4 MCP 与 UI 边界

现有只读 MCP 保持只读。未来 Action MCP 必须是独立的 Admin MCP，不注册到
只读 Bridge。桌面界面分别展示 plan、diff、approval、execution、verification
和 audit link，不使用一个 `success` 标签合并这些状态。

### 3.5 首批排除项

首批不得包含任意 SSH 命令、删除、凭据轮换、IAM 策略修改和数据库迁移。

### 3.6 优势

- 写权限与只读路径在 trait、凭据和 MCP 进程边界上分离。
- 计划 fingerprint、幂等键和回读验证可处理异步 Provider 与重复执行。
- 结构化审批和追加式审计可解释失败，不把 Provider 返回码当成事实。

### 3.7 劣势与风险

- 每个 Provider 都需要独立 action allowlist、幂等语义和验证映射。
- 回读延迟、部分成功和 Provider 状态漂移会使执行结果长期处于验证中。
- 审计事件和 Action secret 的存储策略仍需在实现前进行安全评审。

## 4. 替代方案考量

- **复用 ReadConnector 增加写方法：** 拒绝，因为会破坏只读凭据和 MCP 边界。
- **直接暴露 Provider SDK：** 拒绝，因为无法统一计划、审批、幂等、审计和回读。
- **只依赖人工确认后执行：** 拒绝，因为无法表达计划变更、范围、过期和可重放性。

## 5. 未决问题

- 第一批允许的 Provider action 及其最小写权限分别是什么？
- Action secret 是否使用独立 `connection_secrets` namespace（而非 Keychain）、生命周期和用户确认流程？
- 部分成功时是否提供 Provider-specific compensation，还是只标记待人工恢复？
- 审计事件保留期限、加密方式和导出格式如何定义？

## 6. 实施前门禁

独立评审必须批准状态机、凭据隔离、幂等/重试策略、审计 schema、Provider
action allowlist、回滚策略和失败验证语义。获得新的用户授权前，不实现 Action
Connector、Admin MCP 或任何外部写测试。
