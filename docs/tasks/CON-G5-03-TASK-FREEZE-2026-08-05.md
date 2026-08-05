# CON-G5-03 Workflow、Run 与 Job Mapper 任务冻结

**冻结日期：** 2026-08-05  
**状态：** `FROZEN / IMPLEMENTED / REVIEW`  
**依赖：** `CON-G5-01` 已进入 `REVIEW`  
**独占路径：** `crates/next-infra-connector-github/src/actions/**`、`fixtures/connectors/github/actions/**` 与该模块专属 tests

## 1. 目标

将 GitHub 官方 REST API 的 Workflow、Workflow Run 与 Job allowlisted DTO 映射为稳定、确定、有界的 `ResourceObservation` 与显式 Provider relations。该分支只负责 mapping，不实现 Connector 总编排、Store/Sync、Runtime registry、UI/MCP 或 live token 验收。

官方基线（2026-08-05 查阅）：

- [List repository workflows](https://docs.github.com/en/rest/actions/workflows?apiVersion=2026-03-10)
- [List workflow runs for a repository](https://docs.github.com/en/rest/actions/workflow-runs?apiVersion=2026-03-10)
- [List jobs for a workflow run](https://docs.github.com/en/rest/actions/workflow-jobs?apiVersion=2026-03-10)

三个 endpoint 均使用 `Actions: read` fine-grained repository permission，复用 `CON-G5-01` 的固定 API version、headers、分页、ETag、rate limit 与 SecretValue 边界。

## 2. DTO allowlist

未知字段由 serde 丢弃；DTO 不定义 logs、artifacts、steps、runner name/group/labels、actor、triggering actor、pull requests、commit message、repository object、URLs、token、variables 或 environment。DTO 不进入错误 message 或持久化 raw payload。

### Workflow

只反序列化：`id: u64`、`name: String`、`path: String`、`state: String`、`created_at: String`、`updated_at: String`。

### Workflow Run

只反序列化：`id: u64`、`workflow_id: u64`、`name: Option<String>`、`display_title: String`、`run_number: u64`、`run_attempt: u64`、`event: String`、`status: String`、`conclusion: Option<String>`、`head_branch: Option<String>`、`created_at: String`、`updated_at: String`、`run_started_at: Option<String>`。

不保留 `head_sha`、actor、commit、pull requests 或 URL；这些字段不是首版可视化所必需，并会扩大个人/仓库内容暴露面。

### Job

只反序列化：`id: u64`、`run_id: u64`、`name: String`、`status: String`、`conclusion: Option<String>`、`started_at: Option<String>`、`completed_at: Option<String>`。

不反序列化 `steps` 与 runner 字段。用户自定义 step 名称可能含敏感文本，首版不进入资源摘要。

所有 `String` 字段最大 1,024 bytes；超限或空的必需 name/title 为 module-level invalid response。时间字符串只作为 provider metadata 字符串保存；Observation 的 `observed_at` 必须由调用方注入同一轮 `Timestamp`，不把 provider timestamp 猜成采集时间。

## 3. 稳定身份与 scope

调用方必须提供 `GitHubRepositoryContext { repository_id, repository_external_id, scope }`；repo owner/name 只用于构造 endpoint path，不能进入 external ID、evidence key、错误或 fixture。

| Resource | external_id | name/display |
|---|---|---|
| Workflow | `github-workflow:{id}` | name；display 同 name |
| Run | `github-run:{id}` | `run-{run_number}`；display 优先 `display_title`，否则 workflow name/run number |
| Job | `github-job:{id}` | name；display 同 name |

rerun 不改变 Run external ID；`run_attempt` 作为属性变化形成新 ResourceVersion。Job ID 由 GitHub 提供，不能用 name/顺序推导。

所有资源使用 repository context 的 scope。labels 只允许低基数字段：`github.resource_type`、Run 的 `github.status`、Job 的 `github.status`；不把 branch/event/name 写入 label。

## 4. 属性 schema v1

- `github.workflow`: `workflow_id`、`path`、`state`、`created_at`、`updated_at`。
- `github.workflow_run`: `run_id`、`workflow_id`、`run_number`、`run_attempt`、`event`、`status`、`conclusion`、`head_branch`、`created_at`、`updated_at`、`run_started_at`。
- `github.workflow_job`: `job_id`、`run_id`、`status`、`conclusion`、`started_at`、`completed_at`。

所有 optional 字段缺失时写 JSON null，保持 schema 稳定；未知 Provider 字段不得存活。

Health 映射冻结：

- `status != completed` → `Unknown`。
- completed + `success` → `Healthy`。
- completed + `failure|timed_out|startup_failure|action_required` → `Unhealthy`。
- completed + `cancelled|stale` → `Degraded`。
- completed + `neutral|skipped|null|未知 conclusion` → `Unknown`。
- Workflow health 固定 `Unknown`；state 只作为属性，不推断可运行性。

## 5. Relations

| module | relation | evidence key | field path |
|---|---|---|---|
| `github.repository_workflow` | Repository → Workflow `github.contains` | `github-provider-workflow:{workflow_id}` | `attributes.workflow_id` |
| `github.workflow_run` | Workflow → Run `github.executes` | `github-provider-run:{run_id}` | `attributes.workflow_id` |
| `github.run_job` | Run → Job `github.contains` | `github-provider-job:{job_id}` | `attributes.run_id` |

Run 引用的 workflow ID 不在同一批 Workflow DTO 中时仍可产生 relation locator；endpoint 是否存在由后续 Normalizer/Store 纵切处理，不在 mapper 内猜测或创建伪 Workflow。

## 6. 结果预算与 coverage

- Workflows：每 repository 最多 200 条（最多 2 页）。
- Runs：按 API 默认 newest-first，只取每 repository 最新 100 条（1 页）；这是明确的 bounded current view，不是完整历史。
- Jobs：每 run 最多 200 条；每 repository 每轮最多 2,000 条 Job。
- 达到 Run/Job 结果上限时正常停止，但 mapper output 必须携带 `bounded=true` 和 module gap；合并后的 Connector batch coverage 必须是 `Partial`，不得 authoritative tombstone 历史 Run/Job。
- 任意 endpoint 中途分页/429/permission 失败：保留成功页并返回 module partial；第一页失败且无观察为 fatal module error。
- Workflow 在未触发上限且分页完整时可以报告 module complete；Goal 5 聚合 batch 是否 authoritative 由 `CON-G5-04` 冻结，mapper 不直接生成 Tombstone 语义。

Mapper 输出固定为 `ActionMapperOutput { resources, relations, modules, warnings }`；`modules` 逐项包含 collected count、bounded、complete/partial 与可选结构化 failure。不得直接返回 `SyncOutcome`，避免在分支内建立另一套 Connector orchestration。

## 7. Fixture 与测试

Fixture 路径只使用 `fixture-owner`、`fixture-repo`、数字 synthetic IDs 和 `example.test`（如必须出现 URL）；不得从 live response 录制或复制名称、branch、SHA、runner、actor。

必须验证：

1. workflow/run/job DTO 未知字段以及 secret/runner/steps sentinel 不进入序列化 Observation。
2. 输入顺序变化后 resources/relations 按 `(kind, external_id/evidence_key)` 稳定排序。
3. rerun attempt 只改变属性，不改变 Run external ID/evidence key。
4. relation endpoint、kind、field path 与 evidence key 精确稳定。
5. health 映射覆盖 success/failure/cancelled/pending/未知值。
6. 100 Run、200 Job/Run、2,000 Job/repository 上限，bounded output 不宣称 authoritative。
7. page 2 失败保留 page 1；第一页 401/403/429/failure 无资源时为 fatal module error。
8. Observation 经 Goal 2 Normalizer schema 后未知字段消失、secret sentinel 为 0，并通过公共 conformance ordering/redaction。

验证：

```bash
rtk cargo test -p next-infra-connector-github -- actions
rtk cargo clippy -p next-infra-connector-github --all-targets -- -D warnings
rtk cargo fmt --all --check
```

## 8. 非目标与停止条件

- 不下载 logs/artifacts，不读取 steps output、environment、secrets、variables 或 runner identity。
- 不实现 dispatch、rerun、cancel、approve 或任何 write API。
- 不将 branch、commit SHA、actor、repo full name 作为稳定身份或 label。
- 需要新增 transport query key、扩大权限/页数/body/deadline、修改共享 Connector API/Core/Normalizer 时立即停止并回派 owner。
- GitHub 返回的 endpoint schema 与本文 allowlist 不一致时，先更新官方依据和冻结文档，不能宽松接收完整 payload。

## 9. 实施结果

2026-08-05 已完成纯 Actions DTO/mapper、数量上限、stable identity/relation、health、partial module 状态和 Normalizer/conformance 证据；详见 [`CON-G5-03-2026-08-05.md`](./CON-G5-03-2026-08-05.md)。HTTP collector 与最终 Connector 编排留给 `CON-G5-04`。
