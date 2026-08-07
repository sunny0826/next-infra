# apps/desktop/src — React/TypeScript 前端

## OVERVIEW
Vite + React + vitest 桌面界面；通过 `DesktopAdapter` 接口与 Tauri Host 通信；默认 zh-CN。

## 结构
```
src/
├── app/          # AppShell + routes.ts（overview/inventory/topology/timeline/connectors/settings）
├── features/     # 每页面一个子目录：Page.tsx + .test.tsx + .css（connectors/evidence/inventory/overview/resource-detail/settings/timeline/topology）
├── generated/query/  # ⚠️ ts-rs 生成 DTO（52 个）——禁止手改，改契约后重新生成
├── i18n/         # displayEnum（zh-CN 枚举标签），DEFAULT_LOCALE="zh-CN"
├── platform/desktop-adapter/  # 适配器层（见下）
├── test/fixtures/ # fixture 工厂 + *GoalN*Adapter
├── ui/           # Shell 组件（Navigation/ContextBar/PrimaryCanvas/InspectorHost/RuntimeBar/Icon）
└── styles/shell.css  # 全局 shell 样式；功能样式在 features/*/ 各自 .css
```

## 桌面适配器模式（核心约定）
- `DesktopAdapter` 接口（18 方法）：查询 / binding / GitHub / 运行时。UI 组件只允许经 `useDesktopAdapter()` 访问后端，不得直接 Tauri invoke。
- 实现三态：`RealDesktopAdapter`（Tauri invoke，命令名如 `query_*`/`binding_*`/`github_*`）、`MockDesktopAdapter`（内存快照，测试基类）、`EmptyDesktopAdapter`（浏览器兜底，查询全部抛错）。
- `main.tsx` 用 `"__TAURI_INTERNALS__" in window` 选择 Real vs Empty；无 Host 时 `desktop_transport_failed` 是预期行为，不能替代原生 smoke。
- invalidation：监听 `next-infra://query-invalidated` 事件刷新查询上下文。

## 测试约定
- 组件测试：vitest + Testing Library + `MockDesktopAdapter` 子类 + `DesktopAdapterProvider`；`userEvent.setup()`；`afterEach(cleanup)`。
- Fixture 规则：仅 `fixture-*` 前缀、`example.test`、固定时间戳（`FIXTURE_OBSERVED_AT`）；fixture 校验测试断言无真实 provider 数据（`notMatch(/github\.com|10\.0\.|192\.168\.|secret|password|token/i)`）。
- 领域场景用 `*GoalN*Adapter`（如 `ssh-goal6-adapter.ts`）扩展 MockDesktopAdapter。

## 命令
```bash
rtk pnpm --dir apps/desktop lint     # tsc --noEmit
rtk pnpm --dir apps/desktop test     # 依赖方向检查 + vitest
rtk pnpm --dir apps/desktop build
```

## 禁止
- 手改 `generated/query/**`（ts-rs 输出）。
- 在 UI 层引入业务逻辑或直接 Tauri invoke（一律经 adapter）。
- 在 fixture 中放入真实仓库名/地址/凭据。
