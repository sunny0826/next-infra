# CON-G5-04 GitHub Collector、ReadConnector 与纵切任务冻结

**冻结日期：** 2026-08-05  
**状态：** `FROZEN / IMPLEMENTED / REVIEW`  
**依赖：** `CON-G5-01/02/03` 均处于 `REVIEW`

## 1. 目标

将既有 GitHub transport、Repository mapper 与 Actions mapper 汇合为一个 `ReadConnector`，并提供完全离线的 fake-transport 纵切证据。真实 token、Desktop/MCP live acceptance 按用户决定继续 deferred。

本任务不修改 Core/Connector API/Normalizer/Store/Sync 语义，不执行任何 Provider 写操作。

## 2. Connector 状态与缓存

`GitHubConnector<T, C>` 持有：

- `GitHubClient<T, C>`；
- 进程内 ETag/page cache，key 为 endpoint identity，value 为 first-page ETag 与 completed pages；
- 进程内 `RepositoryRouteContext` cache，供 Targeted mode 精确匹配 numeric Repository identity；
- 不持有 Secret、SecretRef、Store、Keychain、Runtime 或 Query 依赖。

cache 不序列化、不落 SQLite、不进入 Debug。进程重启后 cache 为空：Full 重新请求；Targeted route cache 缺失返回 `InvalidResponse`。没有 cached pages 时绝不发送 ETag；若 Provider/fake 在无 cache 下仍返回 304，则 fatal `InvalidResponse`。

## 3. HTTP 调度与预算

client 新增每次 fetch 的显式 `GitHubFetchBudget { max_pages, max_requests }`，只能收紧 G5-01 的 20 pages/200 requests 硬上限。

Full 顺序固定：

1. `/user/repos`；
2. Repository external ID 排序；
3. 每 repo：Environment → Deployment → Workflow → Run；
4. Run external ID 排序后逐个 Job。

首版顺序执行，请求并发为 1，符合 Descriptor 最大并发 2。整轮全局最多 200 requests；每次 fetch 传入剩余 budget，不能先超额再截断。达到预算时停止后续 module，保留已有观察并返回 partial。

module budgets：Repository 20 pages；Environment 1；Deployment 2；Workflow 2；Run 1；Job 2/run，并执行每 repo 2,000 Job 聚合上限。

## 4. ETag/304

- 仅对有完整 cached pages 的相同 endpoint 发送 first-page ETag。
- 200 pages 成功后原子替换 cache；partial pagination 不覆盖原完整 cache。
- 304 返回 cached pages 参与 mapper，但本轮 `ProviderRequestSummary` 使用 304 请求的 count/elapsed/status，而不是缓存旧 summary。
- 304 无 cache、cache endpoint mismatch 或 cached body 无法按当前 DTO 反序列化均为 `InvalidResponse`。

## 5. Validation 与 Sync

`validate`：检查 connector type/config schema/token 后，以 `/user` 做一次只读认证请求；200 且 JSON object → Valid，401/credential → Invalid auth issue，permission/rate/network → Invalid structured issue。response body 不保存。

`sync`：

- 只支持 Descriptor 冻结的 Full/Targeted；Incremental 返回 `InvalidDomainValue`。
- Full 从 `/user/repos` 建立本轮 route cache；Repository 首页无 completed page 的失败为 fatal。
- Targeted 只接受 `github.repository` locator，并从进程内 exact route cache 取得 path；不得从 external ID 猜 owner/name。
- child module 首页/后续页失败保留 Repository 和其他成功 module；Authentication/Credential failure在任意 endpoint 为 batch fatal。
- child 403/404/429/5xx/network、page budget、global budget均形成 module partial warning。

## 6. Coverage 与 SyncOutcome

首版 Actions Run/Job 是 bounded current view，Deployment Status 也明确 unsupported。由于当前 `ObservationBatch` 只有一个 coverage，Full 和 Targeted 聚合结果统一使用 `SyncCoverage::Partial`，永不贡献 missing evidence 或 Tombstone。

成功但仅因冻结范围受限时，返回 `SyncOutcome::Partial`，failure 固定 `PartialPagination`/non-retryable，message 不含 endpoint 或 route。存在真实 module failure 时优先使用该结构化 failure；warnings 保留其余 module gaps。

resources/relations 在合并后再次稳定排序；duplicate identity/evidence 在进入 Normalizer 前拒绝。`ProviderRequestSummary` 为所有本轮实际 HTTP 请求的 saturating sum；cache pages 不增加 request count。

## 7. Fake 纵切验收

必须覆盖：

1. Full Repo → Workflow → Run → Job，同时包含 Environment/Deployment。
2. 所有固定 headers/token redaction 与请求顺序；不调用 logs/artifacts/status/write endpoints。
3. ETag 200 cache → 第二轮 304 reuse；无 cache 304 fatal。
4. Repository page 1 401/429/network fatal；page 2 failure 保留 page 1 partial。
5. Environment/Deployment/Actions 403/404/429 仅 module partial，其他观察存活。
6. request 200 budget、module page cap、Run/Job item cap均在发出下一请求前停止。
7. Targeted cache hit 成功；cache miss、非 Repository locator、route injection 失败。
8. 结果经公共 conformance、Normalizer 与 Sync pipeline 时 partial 不产生 Tombstone。
9. Secret/header/body/route sentinel 不进入 Observation、warning、failure、summary、Debug 或 committed fixture。

## 8. 非目标与停止条件

- 不引入持久 ETag/cache schema；需要跨重启增量时另立 migration/cursor RFC。
- 不并发 fan-out，不加入 retry sleep；调度器以后可在 Descriptor 上限内升级。
- 不实现 GitHub App/OAuth/GHES、Deployment Status、logs/artifacts、Secrets/Variables。
- 需要扩大权限、预算、query allowlist或共享契约时立即停止并回派。

## 9. 实施结果

2026-08-05 已实现 `GitHubConnector<T, C>`、process-local ETag/page 与 route caches、Full/Targeted sync、validation、全局请求预算、六模块顺序采集和真实 SQLite partial replay。详见 [`CON-G5-04-2026-08-05.md`](./CON-G5-04-2026-08-05.md)。Desktop UI acceptance 与 MCP live acceptance 不在本实现提交中；MCP 继续按用户决定 deferred。
