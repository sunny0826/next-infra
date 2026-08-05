# CON-G5-02 Repository、Environment 与 Deployment Mapper 任务冻结

**冻结日期：** 2026-08-05  
**状态：** `FROZEN / READY-TO-IMPLEMENT`  
**依赖：** `CON-G5-01` 已进入 `REVIEW`  
**Reader Review：** 独立 `luna_worker` 只读审查完成

## 1. 范围与所有权

独占路径：`crates/next-infra-connector-github/src/repository/**`、`environment/**`、`deployment/**`，以及 `fixtures/connectors/github/{repository,environment,deployment}/**` 和专属 tests。

仅映射 Repository、Environment、Deployment；不实现 Actions、Store/Sync、Runtime、UI/MCP、跨 Provider inference 或 live token 验收。

首版明确不请求 Deployment Status endpoint。`Deployment` health 固定 `Unknown`，status coverage 列为 known gap；需要 status 时必须单独冻结 DTO、权限与预算。

## 2. 官方 endpoint 与权限

复用 `CON-G5-01` 的 GitHub REST API `2026-03-10`、同源 Link、ETag、15 秒 deadline、并发 guidance 2、每页 100、整批 200 request budget。

- Repository：`GET /user/repos?visibility=all&affiliation=owner,collaborator,organization_member&sort=full_name&direction=asc&per_page=100`，`Metadata: read`。
- Environment：`GET /repos/{owner}/{repo}/environments?per_page=100`，`Actions: read`。
- Deployment：`GET /repos/{owner}/{repo}/deployments?per_page=100`，`Deployments: read`。

不请求 Contents、Administration、Secrets、Variables 或任何 write permission。

Targeted mode 只能接收已存 Repository numeric ID，并由调用方从可信 Connection route context/cache 取得 owner/repo。不得从 external ID、display name 或用户临时输入猜测 endpoint path；route context 缺失时为 `InvalidResponse`，不能降级为任意 URL。

## 3. DTO allowlist

serde 未知字段丢弃；所有 String 最大 1,024 bytes，必需文本非空，control/private-key/Bearer sentinel 拒绝。

Repository：`id: u64`、`name: String`、`owner.login: String`（仅 transient route）、`visibility: String`、`default_branch: Option<String>`、`archived: bool`、`disabled: bool`、`created_at: String`、`updated_at: String`。

Environment：`id: u64`、`name: String`、`deployment_branch_policy: Option<{ protected_branches: bool, custom_branch_policies: bool }>`。

Deployment：`id: u64`、`environment: Option<String>`、`task: String`、`transient_environment: bool`、`production_environment: bool`、`created_at: String`、`updated_at: String`。

排除 full_name、permissions、topics、language、license、URLs、clone/SSH URLs、owner object 其他字段、SHA/ref、description、creator、payload、status payload、protection reviewers/teams/users、logs、artifacts、secrets 和 variables。owner/login 与 repo name route 不进入 Observation、external ID、evidence、error、summary 或 committed fixture。

## 4. 稳定身份、scope 与 schema

调用方提供 `RepositoryRouteContext { repository_id, repository_external_id, owner, name, scope, observed_at }`；owner/name 仅由 client 构造 path，mapper 只消费 numeric ID、external ID、scope 和 observed_at。

| Resource | external_id | name/display | health |
|---|---|---|---|
| Repository | `github-repository:{id}` | repository name | Unknown |
| Environment | `github-environment:{id}` | environment name | Unknown |
| Deployment | `github-deployment:{id}` | `deployment-{id}`；display 优先 environment，否则同名 | Unknown |

rename、route 变化或输入顺序不得改变 identity。重复 numeric ID 返回 `InvalidResponse`。所有子资源复用 Repository scope 与同轮 observed_at。

schema v1：

- `github.repository`: `repository_id`、`visibility`、`default_branch`、`archived`、`disabled`、`created_at`、`updated_at`。
- `github.environment`: `environment_id`、`repository_id`、`protected_branches`、`custom_branch_policies`。
- `github.deployment`: `deployment_id`、`repository_id`、`environment`、`task`、`transient_environment`、`production_environment`、`created_at`、`updated_at`。

optional 缺失统一写 JSON null。visibility 只接受 `public|private|internal`；不从 archived/disabled/production/transient 等 flags 推断 health。

## 5. Relations

- Repository → Environment：kind `github.contains`，module `github.repository_environment`，evidence `github-provider-environment:{environment_id}`，field path `attributes.repository_id`。
- Repository → Deployment：kind `github.contains`，module `github.repository_deployment`，evidence `github-provider-deployment:{deployment_id}`，field path `attributes.repository_id`。

`repository_id` 是 mapper 注入的非秘密来源字段。不得根据 Deployment 的 environment 文本推导 Deployment → Environment relation；environment 名称不是稳定 foreign key。

## 6. 预算、partial 与 output

- Repository：每 batch 最多 2,000（20 页）。
- Environment：每 repository 最多 100。
- Deployment：每 repository 最多 200（2 页）。
- 遍历按 Repository external ID；子模块固定 Environment 后 Deployment；输出按 kind/external ID/evidence 稳定排序。
- 达到上限或全局 request budget：`bounded=true`、module `Partial`，不得 authoritative tombstone。
- Repository 首页 401/credential/unavailable 或无任何 Repository 观察的失败：batch fatal。
- Environment/Deployment 普通 403：该 module `Partial/PermissionDenied`，保留 Repository 与其他 module。
- page 2+ 的 429、5xx、network、permission：保留 completed pages，module partial；第一页无观察则 module failure。
- child 404：warning + module partial，不生成空资源或 tombstone。
- 304 必须由上层缓存恢复 page；无 cache 为 `InvalidResponse`，不能伪造空页。

输出采用 `RepositoryMapperOutput { resources, relations, modules, warnings, routes }`；routes 只在当前调用栈用于 child fetch，必须自定义 Debug 隐藏 owner/name，不能序列化或进入 Observation。mapper 不直接构造 `SyncOutcome`。

## 7. Fixture 与测试

Fixture 只使用 `fixture-owner`、`fixture-repo`、numeric synthetic IDs 和 `example.test`；不得录制 live response、真实 owner/repo、token、IP 或私有 payload。

必须验证：

1. permissions/URLs/payload/creator/reviewer/team/secret sentinel 未知字段不存活。
2. visibility allowlist、default_branch null、archived/disabled flags。
3. Environment branch-policy booleans与缺失 policy null。
4. Deployment optional environment、task 与 flags，不读取 statuses/payload/SHA/ref。
5. rename/route/输入顺序不改变 identity/evidence；duplicate ID 拒绝。
6. relation kind、endpoint、evidence 与 `attributes.repository_id` 精确。
7. 两页、page-2 failure、403/429/401/404、304 cache miss 和预算边界。
8. bounded/partial 不贡献 authoritative deletion。
9. Normalizer schema、公共 conformance、redaction、request summary 无 URL/body/token。
10. route context Debug/serialization 不暴露 owner/name；Targeted 无 route cache 时失败。

验证：

```bash
rtk cargo test -p next-infra-connector-github -- repository environment deployment
rtk cargo clippy -p next-infra-connector-github --all-targets -- -D warnings
rtk cargo fmt --all --check
rtk cargo test --workspace
rtk pnpm --dir apps/desktop run test:dependency-direction
rtk git diff --check
```

## 8. 停止条件

需要 Deployment Status、额外权限/query key、更大预算、任意 GitHub Enterprise origin、共享 Connector API/Core/Normalizer 修改时立即停止并回派 owner；不得在 mapper 内静默扩围。
