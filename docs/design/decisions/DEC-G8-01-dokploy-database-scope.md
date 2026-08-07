# DEC-G8-01 Dokploy Database 范围冻结

**状态：** Accepted for Goal 8 implementation  
**日期：** 2026-08-06

## 决策

Goal 8 的 Dokploy Connector 仅覆盖 `Project`、`Application`、`Deployment`、`Server` 与 `Domain`。`dokploy.database` 不请求、不反序列化、不映射为资源或关系，并在 connector descriptor 的 coverage 中明确标记为 `unsupported`。

## 原因

项目目标要求所有 Connector 从 DTO 边界排除密码、连接串与 token。Dokploy Database 的控制面数据与连接凭据模型尚未有独立 allowlist、权限和最小字段审查。把它作为“顺带支持”的资源会让 Connector 范围与敏感字段边界同时变得不确定。

## 结果

- Goal 8 可以实施其余只读资源链，不阻塞 Repo → Deployment → Host → DNS 合成回放。
- Descriptor 必须对 `dokploy.database` 返回 `unsupported` 及本决策原因，不能以缺失或空列表假装支持。
- 未来纳入 Database 前，必须新增独立决策，冻结 API 字段 allowlist、最小只读权限、稳定身份、敏感字段拒绝测试和 mapper 覆盖。
- Connector 不下载应用源代码、部署日志、环境变量或任何 secret。
