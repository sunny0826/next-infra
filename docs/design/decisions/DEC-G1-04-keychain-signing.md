# DEC-G1-04：Keychain、签名与发布边界

**状态：** Accepted technical boundary（外部标识与发布凭据仍待用户决定）  
**任务：** `DEC-G1-04`  
**适用范围：** Goal 1 及后续 macOS Desktop Host、SecretProvider、直接分发  
**不构成：** 工程实现、Keychain item 创建、签名、公证或发布授权

## 1. 唯一选择

| 事项 | 决策 |
| --- | --- |
| Keychain | macOS **Data Protection Keychain**；所有 `SecItem` 操作设置 `kSecUseDataProtectionKeychain=true` |
| Item | `kSecClassGenericPassword`；`kSecAttrSynchronizable=false` |
| 可用时机 | `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`；锁屏时不允许新读取 |
| 访问控制 | provisioning profile 授权的显式私有 Keychain access group；不使用 file-based Keychain `SecAccess` ACL |
| 访问进程 | 只有 Desktop Host 内的 Rust SecretProvider；React、MCP Bridge、普通 CLI 均不能读取 |
| 环境隔离 | 开发版使用独立 bundle ID、service、access group，不读取发布 Secret |
| Secret 替换 | 新 generation 写入 → 无 UI 读回 → 原子切换 SecretRef → 删除旧 generation |
| 本地开发 | ad-hoc 只跑 Mock/Fixture；Keychain smoke 必须是 Apple Development 签名 App |
| 外部分发 | Developer ID Application、Hardened Runtime、secure timestamp、notarization、stapling |
| 渠道与更新 | 首版只做直接分发的完整 App/DMG；不做 App Store，不启用 Tauri Updater |

Apple 建议新 macOS 代码优先使用 Data Protection Keychain；其 access group 由代码签名 entitlement 和 provisioning profile 约束。[TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains)、[TN3125](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)

## 2. Keychain 身份与 SecretRef

### 2.1 编译期身份

```text
release_bundle_id = <USER_SELECTED_RELEASE_BUNDLE_ID>
development_bundle_id = <release_bundle_id>.dev
release_access_group = <APPLE_TEAM_ID>.<release_bundle_id>
development_access_group = <APPLE_TEAM_ID>.<development_bundle_id>
```

开发、发布 App 分别声明自己的：

- `com.apple.application-identifier`。
- `com.apple.developer.team-identifier`。
- 只含当前私有组的 `keychain-access-groups`。
- 与 entitlement 匹配的 embedded provisioning profile。

access group 不使用通配符，也不与 MCP Bridge 或 helper 共享。Tauri 通过 `bundle.macOS.entitlements` 使用 entitlement 文件，并可把 profile 放入 `Contents/embedded.provisionprofile`。[Tauri macOS App Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)

### 2.2 Item 命名

```text
service = <current_bundle_id>.provider-secret.v1
account = connection/<connection_uuid>/kind/<secret_kind>/generation/<generation_uuid>
```

- `connection_uuid` 是安装内随机 ID，不使用显示名、主机名、Provider account、仓库名或 IP。
- `secret_kind` 是 Rust 固定枚举，不能由 React 任意指定。
- 每次替换生成新 `generation_uuid`，允许新旧 item 短暂共存。
- `kSecAttrAccessGroup` 总是显式指定；`kSecAttrLabel` 固定为 `Next Infra provider credential`。
- 不存储 `kSecValuePersistentRef`。[generic password identity](https://developer.apple.com/documentation/security/ksecclassgenericpassword)

SQLite 的 SecretRef 只保存：

```text
backend = "macos_data_protection_keychain_v1"
service
account
secret_kind
generation_uuid
created_at
last_verified_at
permission_scope_summary
```

不保存 Secret、可逆密文、Token 片段/hash、persistent reference、profile 或签名/公证凭据。Rust 从当前编译身份推导 access group，并拒绝 service 与当前 bundle ID 不一致的 SecretRef。

## 3. Item 属性与无交互读取

所有 add/read/delete 查询显式包含 class、Data Protection Keychain、`synchronizable=false`、当前 access group、service 和 account；创建时额外使用 `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`。[Apple accessibility](https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly)、[`kSecUseDataProtectionKeychain`](https://developer.apple.com/documentation/security/ksecusedataprotectionkeychain)

后台读取必须：

- 在非 UI 线程调用阻塞的 `SecItemCopyMatching`。[Apple API](https://developer.apple.com/documentation/security/secitemcopymatching(_:_:))
- 使用 `LAContext.interactionNotAllowed=true` 与 `kSecUseAuthenticationContext`，禁止系统授权弹窗。
- Secret 只在一个 SyncRun 所需的最短时间留在内存，结束、取消或错误后清除。

不使用 `userPresence`、Touch ID 或每次读取密码确认，因为它们与无窗口同步不兼容。Keychain 只能阻止锁屏后的新读取，不能追溯撤销已经进入进程内存的值。

## 4. 替换顺序与错误语义

```mermaid
sequenceDiagram
    participant UI as Desktop UI
    participant SP as SecretProvider
    participant KC as Data Protection Keychain
    participant Store as Local Config Store
    UI->>SP: replace(connection_id, kind, secret bytes)
    SP->>KC: add(new generation)
    SP->>KC: read-back(new, no UI)
    SP->>SP: compare in memory; log status only
    SP->>Store: atomically switch SecretRef
    Store-->>SP: committed
    SP->>KC: delete(old generation)
    SP-->>UI: success without Secret
```

失败规则：

1. add/read-back 失败：删除新 item，旧 SecretRef 不变。
2. read-back 不记录明文、长度、hash 或 Token 片段。
3. Store 切换失败：尽力删除新 item，旧 item 保持有效。
4. Store 已提交但旧 item 删除失败：新引用保持 active，记录非秘密 `cleanup_pending`，不回退。
5. 恢复只比较本 service/access group 下的 metadata 与 active SecretRef，不读取 `kSecValueData`。
6. 不用原地 `SecItemUpdate` 替换值。

| 场景 | 结果 | 禁止行为 |
| --- | --- | --- |
| 登录启动、会话已解锁 | 正常按计划读取 | 显示窗口或索要 Secret |
| 锁屏后新 SyncRun | `credential_unavailable`；Resource Health 保持，Freshness 变化 | 弹窗、报 `authentication_failed`、高频重试 |
| Keychain 暂不可用 | `credential_unavailable`，等待有界调度或手动重试 | 循环调用 |
| item 缺失 | `credential_unavailable`，subreason=`missing` | 从 env/shell/旧配置猜测 |
| entitlement/group/profile 错 | `internal`，subreason=`signing_configuration_invalid` | 退回 file-based Keychain |
| 锁屏前请求已取得 Secret | 当前有界请求可完成或取消，随后清内存 | 缓存到下一 SyncRun |

日志只记录 Connection ID、结构化分类和 OSStatus 符号名，不记录 service/account 全值、Secret 或 Provider response。

## 5. 签名与发布边界

### 5.1 开发

- ad-hoc 仅用于 Mock Desktop Adapter、Fixture 和无 Secret 测试，不能宣称 Keychain 已验证。
- Goal 1 Tauri 骨架固定使用 ad-hoc bootstrap bundle ID `dev.guoxudong.next-infra.dev`，不声明 Keychain access group、application identifier 或 team entitlement。它不是下述 Apple Development 身份，未来切换签名配置时不得沿用或迁移其中的 Secret。
- Keychain smoke 使用 `development_bundle_id`、Apple Development certificate、对应 profile 和 dev access group。
- dev/release 双向不可读；从开发切换发布时由用户重新录入，不提供 Secret 迁移工具。

### 5.2 Developer ID 发布

任何离开当前开发机的 artifact 必须：

- 用 `Developer ID Application` 签名 App、嵌套 executable 和 `next-infra-mcp`。
- Desktop Host 携带授权 release group 的 profile；Bridge 不声明 Keychain entitlement。
- 启用 Hardened Runtime 与 secure timestamp，且没有 `com.apple.security.get-task-allow=true`。
- 经 Apple notary service 验证并 staple。[Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)、[Developer ID](https://developer.apple.com/developer-id/)

采用默认 designated requirement；同 Team、bundle ID 和 access group 的连续版本必须通过升级 smoke。自定义 requirement 需重新 Review。[TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)

### 5.3 更新

首版只安装完整、已签名且已公证的 App/DMG。Host 与 Bridge 的原子替换由 `DEC-G1-03` 定义，本决策禁止单独替换其中一个。

首版不加入 `tauri-plugin-updater`、不生成 updater artifact/endpoint/`TAURI_SIGNING_PRIVATE_KEY`。Tauri updater 使用另一套不可禁用验证的签名密钥；启用时必须重开本决策，设计私钥保管、轮换、回滚和 Host/Bridge 原子更新。[Tauri Updater](https://v2.tauri.app/plugin/updater/)

## 6. 威胁边界与非目标

本决策降低 SQLite/日志/shell 中明文泄漏、其他签名读取发布 Secret、iCloud/设备迁移复制、锁屏弹窗/重试风暴，以及未签名或局部更新破坏身份的风险。

不承诺抵御：当前用户完整权限/root/调试注入、被窃取的 Developer ID 私钥、被控制的签名 App/OS、已进入进程内存的 Secret、Provider 泄漏，以及用户主动修改 Keychain。FileVault 只是补充保护。

非目标：

- 本任务不创建或读取 Keychain item、真实 Secret、Developer ID 私钥或公证凭据。
- 不选择 Rust Keychain crate，不实现 Command、SecretProvider、profile 或 CI。
- 不允许 MCP Bridge、React、Agent 或普通 CLI 读取 Secret。
- 不支持锁屏后新建带凭据同步。
- 不设计 App Store、App Sandbox、iCloud Keychain、多用户、headless daemon 或 Keychain sharing。
- 不启用在线更新，不定义 Bridge 安装路径和协议窗口。

## 7. 验证矩阵

Secret smoke 仅使用测试专用、无外部权限的合成字节，测试后删除；输出只有状态和 generation ID。

| 环境 | 场景 | 预期 |
| --- | --- | --- |
| Rust + Mock | add/read/replace/error mapping | 不调用 macOS Keychain |
| ad-hoc App | Fixture/UI | 通过；Secret 路径明确不可用 |
| Apple Development App | add→read→replace→delete | 仅访问 dev namespace，无弹窗 |
| dev/release 交叉读取 | 两个签名 artifact | 双向拒绝 |
| 锁屏后台 | 锁屏后调度读取 | `credential_unavailable`，无弹窗；解锁后有界恢复 |
| Developer ID v1 | 创建 release 测试 item | entitlement/profile/group 一致 |
| 同身份 v1→v2 | 完整替换后读取 | v2 可读，requirement/group 兼容 |
| 错 Team/bundle 负例 | 读取 release 测试 item | 拒绝，不写永久授权 |
| stapled DMG | 干净环境首次启动 | Gatekeeper 接受，App 正常启动 |
| 发布配置 | 扫描 updater | 不存在在线 updater |

锁屏必须由用户执行真实系统锁屏；普通 `cargo test` 不能替代 Apple Development/Developer ID packaged App smoke。

## 8. 可复现验证命令

以下是 Goal 1 实现后的验收契约，本任务不执行签名、公证或 Secret smoke：

```bash
rtk proxy security find-identity -p codesigning -v
rtk proxy xcodebuild -version

NI_APP="/absolute/path/to/Next Infra.app"
NI_BRIDGE="/absolute/path/to/next-infra-mcp"
NI_PROFILE="$NI_APP/Contents/embedded.provisionprofile"
NI_DMG="/absolute/path/to/Next-Infra.dmg"

rtk proxy codesign --verify --strict --verbose=4 "$NI_APP"
rtk proxy codesign -dvvv --entitlements :- "$NI_APP"
rtk proxy codesign -d -r- "$NI_APP"
rtk proxy codesign --verify --strict --verbose=4 "$NI_BRIDGE"
rtk proxy codesign -dvvv --entitlements :- "$NI_BRIDGE"
rtk proxy security cms -D -i "$NI_PROFILE" | rtk proxy plutil -p -

rtk proxy xcrun notarytool submit "$NI_DMG" --keychain-profile "<USER_SELECTED_NOTARY_PROFILE>" --wait
rtk proxy xcrun notarytool log "<SUBMISSION_ID>" --keychain-profile "<USER_SELECTED_NOTARY_PROFILE>"
rtk proxy xcrun stapler validate "$NI_DMG"
rtk proxy spctl -a -vvv --type open "$NI_DMG"
rtk proxy spctl -a -vvv --type execute "$NI_APP"

rtk test pnpm --dir apps/desktop test:keychain-smoke -- --app "$NI_APP" --scenario add-read-replace-delete
rtk proxy rg -n "tauri-plugin-updater|TAURI_SIGNING_PRIVATE_KEY|createUpdaterArtifacts|plugins.*updater" apps crates Cargo.toml
```

验证必须检查 App/Bridge 各自签名、App entitlement/profile/group、Bridge 无 Keychain entitlement、Hardened Runtime、timestamp、无 `com.apple.security.get-task-allow`、notary log、staple、Gatekeeper 和真实首次启动。不得使用 `altool`。公证不替代运行与升级 smoke。[Apple workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)、[Tauri signing](https://v2.tauri.app/distribute/sign/macos/)

## 9. 升级触发条件

以下变化必须重开本决策：

- bundle ID、Team、App ID prefix、access group 或 SecretRef schema 变化。
- helper/Bridge/CLI 共享 Secret，或 Runtime 移出用户登录态 Desktop Host。
- 需要锁屏读取、Touch ID/user presence、App Store/App Sandbox/iCloud/multi-user。
- 启用任意 updater、回滚、updater key rotation 或改变 Host/Bridge 原子策略。
- Apple/Tauri 的 profile、Developer ID、notarization 或 signing 要求变化。

## 10. 必须由用户决定

1. **正式 release bundle ID**：决定 App ID、service 和 release access group，发布后必须稳定。
2. **Apple Developer Team**：确认 Team ID、Account Holder、Developer ID certificate 和 Developer ID profile 能力。
3. **Goal 1 是否完成真实发布门**：若不提供 Team/profile，开发路径可继续，外部分发保持 `BLOCKED`，不能用 ad-hoc 代替。
4. **公证认证方式**：App Store Connect API key 或 Apple ID app-specific password；只能安全托管，不进入仓库、CLI 参数、shell rc 或普通 `.env`。
5. **直接分发确认**：本决策选择 Developer ID DMG 且首版无在线 updater；若要求 App Store/updater，先重开本决策并联合 Review `DEC-G1-03`。

Decision Captain 可在用户选择前合并技术规则，但不能把 Developer ID、公证或发布 Keychain 升级标记为已实际验证。

## 11. 官方依据

- [Apple TN3137：Mac keychain implementations](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains)
- [Apple：Keychain access groups](https://developer.apple.com/documentation/security/sharing-access-to-keychain-items-among-a-collection-of-apps)
- [Apple TN3125：Provisioning Profiles](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)
- [Apple：Keychain accessibility](https://developer.apple.com/documentation/security/restricting-keychain-item-accessibility)
- [Apple TN3127：Code Signing Requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)
- [Apple：Notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [Tauri v2：macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri v2：macOS App Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)
- [Tauri v2：Updater](https://v2.tauri.app/plugin/updater/)
