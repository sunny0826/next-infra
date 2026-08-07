# CON-G8-03 Task Freeze

**状态：** `READY`  
**Owner：** Cloudflare connector worker

## 目标

建立资源限定、只读的 Cloudflare transport、ephemeral token auth 与逐模块 descriptor。

## 独占路径

`crates/next-infra-connector-cloudflare/**`，以及该 crate 的 workspace 注册所需的最小 `Cargo.toml` / `Cargo.lock` 修改。

## 范围

- Account/Zone/DNS/Tunnel/Worker 摘要 endpoint input DTO。
- 明确 Account/Zone scope 与最小 read permission；不保存 token 或 Worker code。
- 固定 base URL validation、cursor pagination 和 Retry-After / 429 fake-test；不得实际请求 Cloudflare。

## 验收与验证

- `rtk cargo test -p next-infra-connector-cloudflare`
- `rtk cargo clippy -p next-infra-connector-cloudflare --all-targets -- -D warnings`
- transport、permission descriptor、header redaction、pagination / rate limit tests 均通过。
