# Next Infra HTML Prototype

这是《界面与可视化设计》的独立交互原型，用于在 Rust、Tauri 和 React 工程实现前验证信息层级、状态语义与核心核实路径。

## 打开方式

直接打开 [`index.html`](./index.html)，或在项目根目录运行：

```bash
python3 -m http.server 4173 --directory prototype
```

然后访问 `http://127.0.0.1:4173`。

原型是一个无远程依赖的单文件 HTML；所有数据都是虚构 Fixture，不读取本机凭据、Provider 或 SQLite，也不会修改任何外部资源。

## 已覆盖的界面

- Overview：Attention Queue、Observation Strip、Critical Path、Recent Changes。
- Inventory：Resource Health、Freshness、Lifecycle 与 Connection 的组合过滤和选择。
- Topology：focus-centered depth、provider/configured/inferred evidence、Frontier 展开和边检查器。
- Timeline：按 SyncRun、Binding、Inference 组织的结构化 Change。
- Connectors：Connector Health、调度、退避、Coverage Matrix 与模拟 Manual Sync。
- Settings：登录启动、MCP 自动拉起、保留期、数据预算、`user_quit` 提示。

全局搜索、资源/关系/Change/Connection 选择都会更新 Evidence Spine。Manual Sync 仅模拟只读 `SyncRun`，不代表未来外部写操作。

## 视觉参考与验证产物

- [`reference/overview-concept.png`](./reference/overview-concept.png)
- [`reference/topology-concept.png`](./reference/topology-concept.png)
- 浏览器截图位于 [`../output/playwright/`](../output/playwright/)

浏览器验收使用 1600×1000、900×800 和 390×844 三种 viewport。窄屏下导航折叠为图标栏，Evidence Spine 变为可关闭抽屉；高密度表格与 Topology 在各自容器内滚动，不扩大页面宽度。

## 明确非目标

- 不创建 React、TypeScript、Tauri 或 Rust 工程骨架。
- 不确定组件库、前端状态库或最终打包方式。
- 不连接真实 Provider、MCP Bridge 或 Desktop Host。
- 不实现任何外部资源写操作。
