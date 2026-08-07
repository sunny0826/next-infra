# CON-G8-01 Task Freeze

**状态：** `READY`  
**Owner：** Dokploy connector worker

## 目标

建立无副作用的 Dokploy transport、ephemeral token auth、descriptor 与严格 DTO allowlist，为后续 mapper 提供安全输入。

## 独占路径

`crates/next-infra-connector-dokploy/**`，以及该 crate 的 workspace 注册所需的最小 `Cargo.toml` / `Cargo.lock` 修改。

## 范围

- 只读 Project/Application/Deployment/Server/Domain DTO。
- 未知字段忽略；password、connection string、token、env、logs 和 raw response 不进入 DTO。
- Database 在 descriptor 标记 `unsupported`，理由引用 DEC-G8-01。
- 固定 base URL validation、分页和 rate-limit fake-test；不得实际请求服务。

## 验收与验证

- `rtk cargo test -p next-infra-connector-dokploy`
- `rtk cargo clippy -p next-infra-connector-dokploy --all-targets -- -D warnings`
- allowlist/redaction、token header sensitivity、transport pagination / 429 tests 均通过。
