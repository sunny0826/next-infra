# Next Infra 结构图

**状态：** Draft（与 RFC-0001 同步）  
**范围：** 首版单机 Tauri 架构的系统上下文、运行时组件、读写路径和生命周期

本文是 [RFC-0001](./RFC-0001-single-node-architecture.md) 的可视化投影。RFC 负责解释决策与约束；本文负责让组件所有权、进程边界和调用方向一眼可见。若两者冲突，以 RFC 为准并同步修正本文。

规范术语见[术语表](./glossary.md)。图中的箭头表示调用或数据方向，不表示所有节点都必须成为独立进程或 Rust crate。

## 1. 一图总览

```mermaid
flowchart LR
    User["当前 macOS 用户"]
    Agents["Codex / Hermes"]

    subgraph Providers["外部基础设施 / Provider 信任边界"]
        ProviderAccess["Read APIs / fixed SSH probes"]
        GitHub["GitHub / Actions"]
        SSH["SSH Hosts / Remote Mac mini"]
        Dokploy["Dokploy"]
        Supabase["Supabase Managed / Self-hosted"]
        Cloudflare["Cloudflare"]
        Clouds["Aliyun / Tencent Cloud"]

        ProviderAccess --- GitHub
        ProviderAccess --- SSH
        ProviderAccess --- Dokploy
        ProviderAccess --- Supabase
        ProviderAccess --- Cloudflare
        ProviderAccess --- Clouds
    end

    subgraph Machine["当前用户的 Mac"]
        subgraph Host["Next Infra.app / Tauri Desktop Host / 单实例"]
            UI["React / TypeScript Desktop UI"]
            Adapter["Desktop Adapter / Tauri Commands + Invalidation Events"]

            subgraph Runtime["Rust Control Plane Runtime / 不依赖 Tauri"]
                Query["Shared Query Service"]
                Sync["Scheduler + Sync Engine"]
                Connectors["Read Connectors"]
                Normalizer["Normalizer"]
                Writer["SQLite Single Writer"]
                LocalRPC["Private Local RPC Server"]
            end
        end

        Bridge["next-infra-mcp / STDIO MCP Bridge / 短生命周期"]
        SQLite[("SQLite Read Model + Limited History")]
        Keychain["macOS Keychain / Secret Values"]
    end

    User --> UI
    UI --> Adapter
    Adapter --> Query
    Adapter -.->|"invalidation only"| UI

    Agents -->|"STDIO MCP"| Bridge
    Bridge -->|"Unix Domain Socket"| LocalRPC
    LocalRPC --> Query

    Sync --> Connectors
    Connectors --> ProviderAccess
    Connectors --> Normalizer
    Normalizer --> Writer
    Connectors -->|"temporary Secret access"| Keychain
    Writer --> SQLite
    Query --> SQLite
```

从这张图应直接读出五个结论：

1. 只有 Desktop Host 是 Next Infra 的长生命周期应用实例。
2. Tauri 负责宿主与适配，不定义领域模型、同步或查询语义。
3. React 与 Agent 最终进入同一个 Query Service。
4. MCP Bridge 不读取 SQLite、Keychain，也不运行 Connector。
5. 外部 Provider 只通过只读 Connector 进入首版系统。

## 2. 进程与所有权边界

```mermaid
flowchart TB
    subgraph ClientProcesses["调用方进程"]
        Codex["Codex process"]
        Hermes["Hermes process"]
    end

    subgraph ShortLived["零到多个短生命周期进程"]
        BridgeA["next-infra-mcp A"]
        BridgeB["next-infra-mcp B"]
    end

    subgraph OneHost["唯一 Next Infra Desktop Host"]
        Tauri["Tauri App lifecycle / tray / windows"]
        WebView["React WebView"]
        RustRuntime["Rust Control Plane Runtime"]
        SocketOwner["Unix Socket owner"]
        DBWriter["SQLite writer owner"]
    end

    SQLite[("next-infra.db")]
    Keychain["Keychain items"]

    Codex --> BridgeA
    Hermes --> BridgeB
    BridgeA --> SocketOwner
    BridgeB --> SocketOwner
    Tauri --> WebView
    Tauri --> RustRuntime
    RustRuntime --> SocketOwner
    RustRuntime --> DBWriter
    DBWriter --> SQLite
    RustRuntime --> Keychain
```

所有权规则：

- 同一用户只有一个 Desktop Host、一个 Control Plane Runtime、一个 Socket owner 和一个 SQLite Writer。
- 可以同时存在多个 MCP Bridge，但它们只是本地查询客户端。
- 操作系统可能为 WebView 创建辅助进程；它们不算第二个 Desktop Host，也不能拥有 SQLite、Socket 或 Keychain 访问边界。
- 第二次启动 Desktop App 只激活现有实例，不能创建新的 Runtime。

## 3. Runtime 组件结构

```mermaid
flowchart LR
    subgraph Inbound["Inbound adapters"]
        DesktopAdapter["Desktop Adapter"]
        LocalRPCAdapter["Local RPC Adapter"]
    end

    subgraph Application["Application services"]
        QueryService["Query Service"]
        ConnectionService["Connection / Local Config Service"]
        Scheduler["Scheduler"]
        SyncEngine["Sync Engine"]
        Normalizer["Normalizer"]
        Writer["Single Writer"]
        Maintenance["Retention / Backup / Recovery"]
    end

    subgraph Domain["Domain core"]
        ResourceModel["Resource / Relation / Version"]
        CoverageModel["Coverage / Freshness / Health"]
        Ports["Connector / Store / Secret ports"]
    end

    subgraph Outbound["Outbound adapters"]
        Connectors["Compiled Read Connectors"]
        Store["SQLite Store"]
        Secrets["macOS Keychain SecretProvider"]
        OpenSSH["System OpenSSH fixed probes"]
    end

    DesktopAdapter --> QueryService
    DesktopAdapter --> ConnectionService
    DesktopAdapter --> Scheduler
    LocalRPCAdapter --> QueryService

    QueryService --> ResourceModel
    QueryService --> Store
    ConnectionService --> Ports
    ConnectionService --> Writer
    ConnectionService --> Secrets
    Scheduler --> SyncEngine
    SyncEngine --> ResourceModel
    SyncEngine --> CoverageModel
    SyncEngine --> Connectors
    Connectors --> Normalizer
    Normalizer --> Writer
    Writer --> Store
    Maintenance --> Store

    Connectors -->|"implements"| Ports
    Store -->|"implements"| Ports
    Secrets -->|"implements"| Ports
    Connectors --> Secrets
    Connectors -->|"SSH Connector only"| OpenSSH
```

边界规则：

- Inbound adapter 只做传输、参数校验、DTO 转换和错误清洗。
- Application service 编排用例，但不把 Provider 特性塞进通用查询协议。
- Domain core 不依赖 Tauri、MCP、SQLite、Keychain 或具体 Provider SDK。
- Outbound adapter 实现 Core 定义的端口；Connector 不能直接持有数据库写连接。
- 本图是逻辑依赖约束，具体 crate 切分在 Goal 1 冻结。

## 4. 同步写入与 UI 失效路径

```mermaid
sequenceDiagram
    participant Scheduler
    participant Connector
    participant Provider
    participant Normalizer
    participant Writer
    participant SQLite
    participant Query as Query Service
    participant Adapter as Desktop Adapter
    participant UI as React UI

    Scheduler->>Connector: SyncRequest(connection, mode, cursor)
    Connector->>Provider: read-only request / SSH fixed probe
    Provider-->>Connector: paginated observations
    Connector-->>Normalizer: ObservationBatch + Coverage
    Normalizer->>Normalizer: allowlist / validate / fingerprint
    Normalizer-->>Writer: ValidatedBatch
    Writer->>SQLite: one transaction
    Writer->>SQLite: projection + versions + relations + changes + cursor + SyncRun
    SQLite-->>Writer: committed
    Writer-->>Adapter: scope/version invalidated
    Adapter-->>UI: invalidation Event only
    UI->>Adapter: bounded Query Command
    Adapter->>Query: validated query DTO
    Query->>SQLite: read committed projection
    SQLite-->>Query: bounded rows
    Query-->>Adapter: versioned response DTO
    Adapter-->>UI: committed versioned snapshot
```

关键约束：

- Event 不携带权威资源列表，只告诉 UI 哪一类查询已经失效。
- UI 和 MCP 永远只读取已提交快照，不观察半个同步批次。
- cursor 与 SyncRun completion 必须和资源投影在同一事务中提交。
- 未变化资源只更新 `last_seen_at`，不制造新 ResourceVersion。

## 5. Desktop 与 Agent 查询汇合

```mermaid
flowchart LR
    subgraph DesktopPath["Desktop query path"]
        React["React screen"] --> Command["bounded Tauri Command"]
    end

    subgraph AgentPath["Agent query path"]
        Agent["Codex / Hermes"] --> MCPTool["read-only MCP Tool"]
        MCPTool --> Bridge["MCP Bridge"]
        Bridge --> RPC["versioned Unix Socket request"]
    end

    Command --> Query["Shared Query Service"]
    RPC --> Query
    Query -->|"bounded read"| Snapshot[("SQLite committed snapshot")]
    Snapshot -->|"committed rows"| Query
    Query --> Boundary["field filtering / pagination / topology limits / error cleaning"]
    Boundary --> DesktopDTO["Desktop response DTO"]
    Boundary --> MCPContent["MCP Content / Resource"]
```

“共享查询服务”不代表 Desktop DTO 与 MCP Content 必须逐字相同；两者可以有不同传输投影，但字段语义、Freshness、Coverage、分页和硬上限必须来自同一 Query Service。

## 6. Desktop Host 生命周期

```mermaid
stateDiagram-v2
    [*] --> NotRunning

    NotRunning --> Starting: user launch
    NotRunning --> Starting: enabled login autostart
    NotRunning --> Starting: authorized MCP launch and no user_quit

    Starting --> WindowVisible: interactive launch
    Starting --> BackgroundOnly: login or MCP background launch

    WindowVisible --> WindowHidden: close main window
    WindowHidden --> WindowVisible: Dock / tray / second launch
    BackgroundOnly --> WindowVisible: user activates app
    WindowVisible --> WindowHidden: hide

    WindowVisible --> GracefulExit: explicit Quit
    WindowHidden --> GracefulExit: explicit Quit
    BackgroundOnly --> GracefulExit: explicit Quit
    GracefulExit --> UserQuitLatched: drain Writer and close Socket

    UserQuitLatched --> UserQuitLatched: MCP call returns host_unavailable
    UserQuitLatched --> Starting: user launch clears latch
    UserQuitLatched --> Starting: next enabled login autostart clears latch

    WindowVisible --> NotRunning: crash / OS shutdown
    WindowHidden --> NotRunning: crash / OS shutdown
    BackgroundOnly --> NotRunning: crash / OS shutdown
```

- 关闭窗口是隐藏，不是退出。
- single-instance plugin 必须最先注册；BackgroundOnly 初始不创建 WebView，登录启动和 MCP 启动都不能把参数当授权。
- `user_quit` 只由显式退出写入；崩溃、关机和升级重启不写入。
- MCP 自动拉起不能清除 `user_quit`。
- 崩溃恢复依靠 WAL、事务、interrupted SyncRun 和 cursor 提交规则，而不是假装上次同步成功。
- 完整事件映射与验收矩阵见 [`DEC-G1-02`](./decisions/DEC-G1-02-desktop-lifecycle.md)。

## 7. Goal 1 工程依赖方向

以下依赖方向与 package 边界已由 [`DEC-G1-01`](./decisions/DEC-G1-01-toolchain-and-crates.md) 冻结：

```mermaid
flowchart TD
    DesktopApp["apps/desktop / composition root"]
    BridgeApp["apps/mcp-bridge / STDIO executable"]
    Tauri["Tauri v2"]
    MCPLogic["next-infra-mcp"]
    LocalRPC["next-infra-local-rpc"]
    Query["next-infra-query"]
    Runtime["next-infra-runtime"]
    Sync["next-infra-sync"]
    Catalog["next-infra-connector-catalog"]
    API["next-infra-connector-api"]
    Normalizer["next-infra-normalizer"]
    Fixture["next-infra-connector-fixture"]
    ContractTests["next-infra-connector-contract-tests"]
    Store["next-infra-store"]
    Core["next-infra-core"]

    DesktopApp --> Tauri
    DesktopApp --> Runtime
    DesktopApp --> Query
    DesktopApp --> LocalRPC
    BridgeApp --> MCPLogic
    MCPLogic --> LocalRPC
    LocalRPC --> Query
    Runtime --> Store
    Runtime --> Sync
    Runtime --> Query
    Runtime --> Catalog
    Query --> Core
    Sync --> Core
    Sync --> API
    Sync --> Normalizer
    Catalog --> API
    Normalizer --> API
    Normalizer --> Core
    API --> Core
    Fixture --> API
    Fixture --> Core
    ContractTests -. test-only .-> API
    ContractTests -. test-only .-> Fixture
    ContractTests -. test-only .-> Normalizer
    Store --> Core
```

禁止方向：

```text
core/store/sync/query/runtime/local-rpc/mcp/connector-* -> Tauri
React -> SQLite / Keychain / Provider SDK / system shell
MCP Bridge -> SQLite / Keychain / Connector
Connector -> SQLite writer
```

`apps/mcp-bridge` 是独立 Cargo package 和 `next-infra-mcp` binary；它不得成为 Tauri sidecar、Desktop binary target 或 App Bundle 内容。

## 8. 图的变更规则

- 新增长生命周期进程、网络监听端口、数据库 owner 或信任边界时，必须先修改 RFC，再修改本文。
- 新增 Connector 通常只扩展 Provider 与 Outbound adapter，不应改变 Desktop/MCP 查询汇合点。
- 新增写操作必须进入独立 RFC；不得把只读箭头悄悄改成双向控制箭头。
- 图中术语必须来自[术语表](./glossary.md)，不得为同一组件创建新的别名。
