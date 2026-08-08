# DEC-G1-04：签名与发布边界

**状态：** Partially Superseded（2026-08-07 用户决策）
**任务：** `DEC-G1-04`
**修订日期：** 2026-08-07

## Keychain 方向已取消（2026-08-07 用户决策）

**Keychain 相关内容已全部移除。**

用户决定（2026-08-07）：Secrets 一律存 SQLite `connection_secrets` 表（plaintext BLOB，0600 DB/0700 目录、FK 级联清理），不追求 Keychain 迁移。`apps/desktop/src-tauri/src/keychain/` 模块已删除，`pub mod keychain;` 已从 `lib.rs` 移除。

以下签名/发布边界部分仍有效，是未来发布的必要条件（Developer ID、codesign、notarization 仍是未来 gate）。

---

## 签名与发布边界（仍有效）

### 1. 外部分发要求

任何离开当前开发机的 artifact 必须：

- 用 `Developer ID Application` 签名 App、嵌套 executable 和 `next-infra-mcp`。
- 启用 Hardened Runtime 与 secure timestamp，且没有 `com.apple.security.get-task-allow=true`。
- 经 Apple notary service 验证并 staple。

### 2. 更新策略

首版只安装完整、已签名且已公证的 App/DMG。首版不启用 Tauri Updater。

### 3. 威胁边界

不承诺抵御：被窃取的 Developer ID 私钥、被控制的签名 App/OS。

### 4. 必须由用户决定

1. **正式 release bundle ID**：决定 App ID，发布后必须稳定。
2. **Apple Developer Team**：确认 Team ID、Account Holder、Developer ID certificate。
3. **公证认证方式**：App Store Connect API key 或 Apple ID app-specific password。

---

**原 DEC-G1-04 完整内容（已归档）：** 本文件于 2026-08-07 被重写为简短修订注记，原完整 Keychain 技术规范已归档，不再适用。

原始文件路径（已删除模块）：`apps/desktop/src-tauri/src/keychain/mod.rs`、`apps/desktop/src-tauri/src/keychain/platform.rs`。
