# MREL 手工跨 Provider 关系任务冻结

- **状态：** DONE（2026-08-09；Gate PASS）
- **日期：** 2026-08-09
- **权威合同：** [`DEC-G7-02`](../design/decisions/DEC-G7-02-manual-cross-provider-relations.md)
- **授权：** 用户已明确要求开始执行本任务集。

## 1. 目标与共同边界

交付可从 Topology 和 Resource Inspector 创建、编辑和 disable 跨 Provider configured Binding 的完整本地路径，并覆盖 Supabase Self-hosted、Dokploy、Tencent、SSH、GitHub、Cloudflare 与 Supabase Managed 的合成验收。关系构建器使用独立弹窗，不嵌入 Evidence Spine。

所有 worker 必须：

- 只修改声明的独占路径，不回滚其他 worker 的改动。
- 所有 shell 命令使用 `rtk`；Cargo 一律 `--locked`。
- 保持 Provider 只读，不读取网络、环境变量、Secret 或真实 Provider 数据。
- 不手改 `apps/desktop/src/generated/**`，不修改 Core、Store migration、QDTO、root manifest/lockfile；Tauri composition 不新增命令，仅允许修正既有 Binding 命令的安全错误分类。
- 需要越过独占路径或改变 `DEC-G7-02` 时立即停止并回报。

## 2. 执行波次与所有权

### Wave 1（并行）

| Task | 独占路径 | 唯一结果 |
| --- | --- | --- |
| `MREL-01` | `crates/next-infra-connector-supabase-self-hosted/**` | `supabase.self_hosted.instance` 与 `supabase.contains` table relation |
| `MREL-02` | `crates/next-infra-binding/**` | 跨 Connection、重复守卫、自环、unresolved/recovery/disable 回归 |
| `MREL-03` | 新建 `apps/desktop/src/test/fixtures/manual-relation-adapter.ts` 及对应测试/唯一 export | 跨 Provider stateful UI fixture |

### Wave 2（并行，等待 Wave 1）

| Task | 独占路径 | 唯一结果 |
| --- | --- | --- |
| `MREL-04` | 新建 `apps/desktop/src/features/topology/manual-relations/**` | 关系词汇、全局资源选择与 RelationBuilder |
| `MREL-05` | `TopologyPage.tsx`、`TopologyPage.test.tsx`、`topology.css` | configured 呈现、placeholder、create/edit 回调，移除内联表单 |
| `MREL-06` | 新建 `tests/integration/topology/repo-deployment-host-dns/tests/manual_cross_provider_bindings.rs` | 多 Connection Binding/Topology Rust 回归 |

### Wave 3（串行集成）

`MREL-07` 由唯一 Shell Owner 修改：

- `apps/desktop/src/ui/InspectorHost.tsx`
- `apps/desktop/src/ui/PrimaryCanvas.tsx`
- `apps/desktop/src/app/AppShell.tsx`
- `apps/desktop/src/main.test.tsx`

它负责以独立 RelationDialog 承载 RelationBuilder、由 Topology 与 Resource Inspector 传递 create/edit 请求，并在 mutation 后清理旧选择、推进 `queryVersion`。

### Wave 4（并行 QA）

| Task | 独占路径 | 唯一结果 |
| --- | --- | --- |
| `MREL-08` | 新建 `apps/desktop/tests/acceptance/topology-binding/**` | 完整 UI 用户路径、键盘、响应式与无 Secret DOM |
| `MREL-09` | 新建 `tests/integration/topology/repo-deployment-host-dns/tests/manual_relation_safety.rs` | Fixture/provenance/无网络与敏感数据安全回归 |

`MREL-10` 由 Gate Captain 串行运行全量验证并写 Gate 报告；不借 Gate 修复生产代码，失败回派原 Owner。

## 3. 行为验收

- `supabase.self_hosted.instance -> infra.deployed_via -> dokploy.application/project`。
- `tencent.cvm.instance -> infra.accessed_via -> ssh.host`，不合并 identity。
- `github.workflow -> automation.deploys_to -> dokploy.application`，以及 `cloudflare.dns_record -> network.routes_to -> dokploy.domain/application`。
- `github.workflow -> data.writes_to -> supabase.managed.project` 只显示为人工声明。
- source/target 可来自不同 Connection；相同 active/unresolved triple 不重复。
- create/update/disable 后重新查询能观察到 configured evidence；unresolved endpoint 保留 placeholder。
- provider/inferred relation 没有写入口；Topology 边界保持有界。

## 4. 验证

各 Feature Owner 执行任务内 focused tests、lint/clippy、format 与 `rtk git diff --check`。Gate Captain 最终执行：

```bash
rtk cargo test --workspace --all-targets --locked
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo fmt --all -- --check
rtk pnpm --dir apps/desktop test
rtk pnpm --dir apps/desktop lint
rtk pnpm --dir apps/desktop build
rtk git diff --check
```

浏览器 Fixture、单元测试或集成 replay 不得写成真实 Provider、MCP 或原生 App live smoke 通过。

## 5. Handoff

每个 worker 必须报告：修改文件、实现行为、验证命令及结果、未验证项、基线失败、风险和需要回派的契约问题。
