# DEC-G7-01 Binding、Inference、Topology 与 Timeline 合同

- **状态：** Accepted for Goal 7 implementation
- **日期：** 2026-08-06
- **关联任务：** `RHM-G7-01..08`、`UI-G7-01/02`、`GATE-G7`

## 1. 决策范围

Goal 7 在既有 Provider relation 之上加入人工 Binding 与可解释 Inference，但不合并 Resource identity。三类 evidence 可在同一 source/target/kind 上并存，唯一键继续包含 `evidence_type + evidence_key`。

## 2. Binding 合同

- Binding 使用本地生成、不可从 endpoint/display name 推导的 `BindingId`。
- endpoint 只引用已持久化 `ResourceId`；source 与 target 不得相同。
- 状态固定为 `active | unresolved | disabled`。
- create 仅创建 configured evidence；update 只允许修改 kind 或 endpoints，并生成结构化 Change；disable 保留 Binding 与审计记录，同时 tombstone 对应 configured relation。
- 任一 endpoint 不存在或 lifecycle 非 Active 时状态为 unresolved；endpoint 恢复后可回到 active。不得静默删除或改写 Provider/inferred relation。
- Binding 不等于 Resource merge，不改变 connection、external ID、health、freshness 或 Provider attributes。

## 3. Inference 合同

- Inference rule 必须有稳定 `RuleVersion`，输出必须包含 source/target/kind、input resource version IDs、input relation version IDs 与 `Confidence`。
- 相同 rule version 和相同排序后 inputs 必须产生相同 evidence key 与 relation identity。
- 空 inputs、缺失 input version、自环或未注册 rule 拒绝；不得仅以时间邻近、名称相似或 display order 推断因果。
- 输入失效时 inferred relation tombstone，但历史 relation version、Change 与 provenance 保留。
- Provider crate 不直接写跨 Provider inferred relation；候选事实由独立 inference rule 读取 normalized versions。

## 4. SQLite 与兼容性

Goal 2 schema v1 已包含 `bindings`，Goal 7 migration 不重复建表。新 migration 只允许：

- 为 Binding endpoint/status 查询增加 index；
- 增加 inference run/output projection 与唯一性约束；
- 保留旧 resources、versions、relations、bindings、changes 和 sync runs；
- 从 schema v2 原子升级，失败不前移 `user_version`。

旧 `BindingStatus` 只有 active/unresolved；新增 disabled 是向后兼容 serde/SQLite 值。旧 inference origin 缺少 relation inputs，反序列化默认空列表；新写入必须显式保存两类 inputs。

## 5. Query/QDTO 合同

- Binding DTO：ID、endpoints、kind、status、created/updated time；commands 只允许 create/update/disable，不接受 Provider payload。
- Topology 默认 depth 1、100 nodes/200 edges；硬上限 depth 3、200 nodes/400 edges。stable order、`truncated` 和 typed frontier 由 Query Service 强制，客户端不能 load-all bypass。
- Timeline 是 Change 的有界投影，不是 log：默认 50、硬上限 200，opaque cursor；只包含实际 persisted Change，不显示 unchanged poll。
- Timeline item 按 `sync_run | binding | inference` origin 分组，保留 subject、field diffs、resource/relation version links、absolute timestamp 和 evidence。large before/after 仍是结构化 JSON，由 UI 默认折叠。

## 6. UI 合同

- Topology 保留 provider/configured/inferred 视觉与 evidence inspector；节点键盘方向键只在当前 bounded adjacency 内移动，Enter/Space inspect。
- frontier expansion 必须发起下一次 bounded query，不能在客户端递归抓全图。
- Binding create/update/disable 只能走 Desktop Adapter command；UI 不直接修改 relation state。
- Timeline 使用 620px 有界滚动区域、显式 Load more、absolute time 与 origin/version links；无 terminal、raw log 或 infinite scroll。

## 7. 非目标与验收

不实现自动 merge、基于时间/名称猜因果、外部 Provider 写操作、MCP 写工具或无限图。验收必须覆盖同 endpoints 多 evidence、disabled/unresolved/recovery、deterministic inference、migration v2 upgrade、topology hard limits、timeline stable cursor/no unchanged poll、keyboard adjacency 和 replay。
