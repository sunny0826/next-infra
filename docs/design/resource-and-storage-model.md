# 资源与存储模型

本文定义 Next Infra 的资源身份、当前投影、历史版本、关系、同步覆盖和 SQLite 存储语义。实现不得绕过这些规则直接把 Provider JSON 写入数据库。

## 1. 设计原则

1. Provider 是外部事实来源，SQLite 是本地物化视图。
2. 每条资源都必须能追溯到 Connection 与 SyncRun；每条关系必须按来源追溯到 Provider SyncRun、Binding 或可重放的推断输入。
3. 未观察到不等于已删除；只有权威完整扫描才能产生缺失证据。
4. 跨 Connection 的同名、同 IP 资源不自动合并。
5. 核心字段强类型，Provider 扩展属性带 namespace 和 schema version。
6. 原始响应默认不落盘，秘密字段在进入领域层前删除。
7. 只在语义变化时保存新版本，避免轮询制造重复历史。

## 2. 核心实体

### 2.1 Connection

表示一个独立的数据源实例，例如一个 GitHub Account、一套 Dokploy、一组 SSH Host 或一个 Cloudflare Account。

核心字段：

- `connection_id`：安装内稳定的随机 ID。
- `connector_type`：如 `github`、`ssh`、`dokploy`。
- `display_name`：用户可读名称。
- `enabled`：是否参与调度。
- `config`：非秘密配置，经过 schema 验证。
- `secret_ref`：指向 SQLite `connection_secrets` 表的行，不包含秘密本身。
- `health`：Connector 自身状态。
- `last_success_at`、`last_attempt_at`。
- `config_schema_version`。

Connection 删除前必须明确处理其资源：首版采用软删除，并保留资源为 `orphaned`，直到用户执行本地清理。删除 Connection 不能修改外部基础设施。

### 2.2 Resource

表示当前已知的外部资源投影。

唯一键：

```text
(connection_id, kind, external_id)
```

推荐 URI：

```text
infra://resource/<resource-id>
```

推荐可读 URN：

```text
urn:next-infra:<connector>:<connection-id>:<scope>:<kind>:<external-id>
```

核心字段：

- `resource_id`：内部不可变 ID。
- `connection_id`。
- `kind`：命名空间类型，如 `github.repository`。
- `external_id`：Provider 稳定 ID；URL 和名称不能替代稳定 ID。
- `name`、`display_name`。
- `scope`：组织、账户、区域、项目等 Provider 作用域。
- `labels`：经过清洗、带 namespace 的非秘密标签，用于搜索和过滤；不能把任意 Provider 字段伪装成标签。
- `lifecycle`：`active | tombstoned | orphaned`。
- `health`：`healthy | degraded | unhealthy | unknown`。
- `attributes`：已清洗的规范化 JSON。
- `attribute_schema_version`。
- `fingerprint`：规范化后语义内容的稳定摘要。
- `first_seen_at`、`last_seen_at`、`last_changed_at`。
- `last_sync_run_id`。

资源的 `health` 与数据的 `freshness` 分离。Connector 失联时，已有资源不会被标成 `unhealthy`，而是保持最后状态并显示 `stale`。

### 2.3 ResourceVersion

资源只有在 `fingerprint` 变化时才创建不可变版本。正常轮询但内容未变化时只更新 Resource 的 `last_seen_at`。

字段：

- `version_id`、`resource_id`。
- `observed_at`、`sync_run_id`。
- `normalized_snapshot`：包含当时的规范化核心字段、labels 与 attributes。
- `fingerprint`。
- `schema_version`。
- `change_summary`。

Provider 原始响应不属于 ResourceVersion。诊断时需要原始内容，应按需重新读取，并在 UI 明确标为临时数据。

### 2.4 Relation

关系用于表达资源图，例如：

```text
github.repository --defines--> github.actions.workflow
github.actions.run --deploys_to--> dokploy.application
dokploy.application --runs_on--> ssh.host
dokploy.application --depends_on--> supabase.managed_project
cloudflare.dns_record --routes_to--> dokploy.application
aliyun.ecs.instance --represented_by--> ssh.host
```

字段：

- `relation_id`。
- `source_resource_id`、`target_resource_id`。
- `kind`。
- `evidence_type`：`provider | configured | inferred`。
- `evidence_key`：稳定来源身份；同一证据的重复同步不会创建重复关系。
- `evidence_ref`：最新来源证明；Provider 关系引用 Connection、SyncRun 和字段路径，configured 关系引用 Binding，inferred 关系引用规则版本与输入 ResourceVersion。
- `confidence`：仅用于 inferred；Provider 和人工配置不伪装成概率。
- `first_seen_at`、`last_seen_at`。
- `lifecycle`。
- `last_sync_run_id?`：仅 Provider 直接观察到的关系使用；configured 与 inferred 不能伪造 SyncRun。

推荐稳定键：

```text
(source_resource_id, target_resource_id, kind, evidence_type, evidence_key)
```

同一资源对可以同时拥有 Provider、Binding 和推断证据；UI 合并展示时仍必须保留每条证据。

人工配置关系不受 Connector 缺失清理影响。推断关系必须在 UI 中显示证据，不得与 Provider 明确关系使用同一视觉语义。

### 2.5 RelationVersion

Relation 的 kind、端点、证据、confidence 或 lifecycle 发生语义变化时创建不可变 RelationVersion。只有 `last_seen_at` 更新时不创建新版本。

字段：

- `relation_version_id`、`relation_id`。
- `observed_at`。
- `normalized_snapshot`。
- `fingerprint`、`schema_version`。
- `origin_type`：`sync_run | binding | inference`。
- `origin_ref`：对应 SyncRun、Binding 或推断规则与输入版本引用。

### 2.6 Binding

Binding 是用户对跨平台关系的明确声明，用于补足无法自动发现的链路。

示例：

```text
github.repository(owner/repo)
  --deploys_to-->
dokploy.application(app-id)
```

Binding 只创建本地 Relation，不修改外部系统。Binding 端点资源消失时，Binding 保留但标记为 unresolved。

### 2.7 SyncRun

记录一次 Connection 同步尝试：

- `sync_run_id`、`connection_id`。
- `mode`：`full | incremental | targeted`。
- `trigger`：`schedule | user | startup | recovery`。
- `started_at`、`finished_at`。
- `status`：`running | succeeded | partial | failed | cancelled | interrupted`；`interrupted` 仅由启动恢复把遗留 running run 转换而来。
- `coverage`。
- `cursor_before`、`cursor_after`。
- 读取、创建、更新、未变化、警告数量。
- 结构化错误分类，不保存秘密和完整响应体。

### 2.8 Change

Change 是相邻 ResourceVersion 或 RelationVersion 的结构化差异。字段变化采用稳定 path 表示，例如：

```text
attributes.default_branch
attributes.workflow.state
health
lifecycle
```

Change 存储经过清洗的 before/after 摘要。大字段、日志、证书正文和秘密不进入 diff。

每条 Change 使用 `origin_type` 与 `origin_ref` 指向 SyncRun、Binding 或推断计算，不能为本地配置变化伪造 SyncRun。

### 2.9 Capability

Capability 描述资源理论上支持的能力。首版只有只读能力，例如：

- `inspect.summary`
- `inspect.topology`
- `inspect.versions`
- `inspect.provider_details`

未来写能力不得直接混入 Resource 字段，而应由独立 Action Connector 注册带输入 schema、风险级别和验证策略的 Capability。

## 3. 类型命名规则

- 使用小写命名空间：`provider.domain.kind`。
- 不使用一个不断增长的 Rust 大枚举表达所有 Provider 类型。
- Core 可以对少数跨平台类别建模：`compute`、`repository`、`workflow`、`deployment`、`database`、`dns`、`network`。
- Provider 属性只能位于对应 namespace，禁止不同 Connector 复用同名字段但表达不同语义。
- 每个 Connector 对输出 schema 负责，并提供从旧 schema 到当前 schema 的读取迁移。

## 4. 同步覆盖与删除语义

### 4.1 Coverage

同步批次必须返回以下之一：

- `authoritative_full(scope)`：完整枚举了指定 scope。
- `incremental(cursor)`：只包含从 cursor 之后的变化，不能证明其他资源不存在。
- `partial(scope, reason)`：受权限、分页、限流或错误影响，不完整。
- `targeted(resource_ids)`：只刷新指定资源。

只有成功的 `authoritative_full` 可以增加资源的连续缺失计数。

### 4.2 Tombstone

默认规则：资源连续两次未出现在成功、权威、相同 scope 的全量同步中，才转为 `tombstoned`。

以下情况不得增加缺失计数：

- 同步失败或取消。
- 认证失败、限流或分页未完成。
- 权限范围缩小但尚未由用户确认。
- 增量或 targeted 同步。
- Connector 版本升级导致的 kind/schema 迁移窗口。

资源重新出现时恢复为 `active`，保留之前历史。

## 5. 新鲜度与健康

Freshness 由 Connection 的计划间隔和 `last_seen_at` 计算：

- `fresh`：处于预期间隔内。
- `stale`：超过一个容忍窗口。
- `expired`：长时间未成功观察，不再用于健康聚合。

Connector Health 单独表示：

- `healthy`
- `degraded`
- `auth_failed`
- `rate_limited`
- `unreachable`
- `disabled`

UI 和 MCP 必须同时返回 Resource Health、Freshness 和 `observed_at`，不能只返回一个颜色或布尔值。

## 6. SQLite 逻辑表

首版建议表：

```text
connections
connector_state
sync_runs
resources
resource_versions
relations
relation_versions
bindings
changes
schema_migrations
maintenance_runs
```

核心索引：

- Resource 唯一键 `(connection_id, kind, external_id)`。
- Relation 唯一键 `(source_resource_id, target_resource_id, kind, evidence_type, evidence_key)`。
- `resources(kind, lifecycle, health)`。
- `resources(last_seen_at)`。
- `resource_versions(resource_id, observed_at desc)`。
- `relations(source_resource_id, lifecycle)`。
- `relations(target_resource_id, lifecycle)`。
- `changes(observed_at desc)`。
- `sync_runs(connection_id, started_at desc)`。

首版不依赖 FTS5。资源名称、kind、scope、标签和少量规范化字段使用普通索引；模糊查询必须有结果上限。FTS5 只有在应用自带 SQLite 构建并通过启动自检后才可启用。

## 7. 写入和事务边界

- Connector 可并行执行网络读取，但不能持有数据库写连接。
- Connector 输出 `ObservationBatch` 后，由 Normalizer 完成清洗和 schema 验证。
- 一个 Writer 任务按 Connection/SyncRun 串行提交事务。
- Resource、Version、Relation、Change、SyncRun completion 和 cursor 必须在同一事务中一致提交。
- cursor 只在事务成功后前移。
- UI 和 MCP 只读取已提交快照，不观察半个同步批次。

## 8. 数据保留和维护

默认值：

| 数据 | 默认保留 |
| --- | --- |
| 当前 Resource / Relation | 保留；tombstoned 资源以后由用户执行本地清理 |
| ResourceVersion / RelationVersion | 30 天 |
| Change | 180 天 |
| SyncRun | 30 天 |
| 应用日志 | 30 天并限制总大小 |
| Provider 原始响应 | 不保存 |
| GitHub Actions / 部署日志 | 不保存，只按需读取 |

维护任务：

- 定期删除过期历史。
- 执行 WAL checkpoint。
- 使用增量 vacuum 或受控维护窗口回收空间。
- 提供 `VACUUM INTO` 风格的一致本地备份。
- 监控 SQLite、WAL、日志和备份总大小。
- 1 GiB 是软预算；达到 70% 告警，达到 90% 优先清理过期历史并暂停非必要历史写入。

## 9. 敏感数据规则

任何字段进入 Normalizer 前都必须按 Connector 白名单选择。以下内容默认禁止落盘：

- API Token、PAT、Access Key、Secret Key、JWT、Cookie。
- SSH 私钥、口令、passphrase。
- 数据库密码、连接串中的凭据。
- GitHub Actions Secret 值。
- 完整日志和可能包含秘密的环境变量。
- Dokploy API 返回的数据库密码等敏感属性。

未知字段默认丢弃，而不是默认保存。

## 10. 验收条件

- 同一未变化资源重复同步不会增加 ResourceVersion。
- partial、incremental 和 failed run 不会 tombstone 未出现资源。
- 每个 Resource/ResourceVersion 可追溯到 Connection 与 SyncRun；每个 Relation/RelationVersion/Change 可按来源追溯到 SyncRun、Binding 或推断输入。
- Freshness 与 Health 分离并对 UI/MCP 可见。
- 未知 Provider 字段不会绕过白名单进入数据库。
- 删除一个 Connection 不会调用 Provider 删除 API。
