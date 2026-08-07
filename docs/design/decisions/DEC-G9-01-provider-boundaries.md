# DEC-G9-01: Goal 9 Provider Boundaries

Date: 2026-08-06
Status: Accepted for local read-only implementation

## Decision

Goal 9 exposes provider coverage as independent modules. A provider brand is
never represented by one aggregate `supported` value. Every descriptor keeps
connector coverage, sync coverage, and connection health as separate facts.

Supabase has two connector identities:

- `supabase-managed`: Management API organization and project summaries only.
- `supabase-self-hosted`: service API, PostgreSQL metadata, and the fixed SSH
  probe registry as independent source adapters. Managed DTOs are not reused.

Self-hosted resources use the `supabase.self_hosted.*` kind prefix and include
the source in their evidence. Managed resources use `supabase.managed.*`.

Aliyun and Tencent keep region in the connection scope and use stable provider
IDs for identities. The first module list is deliberately bounded:

| Connector | Modules |
| --- | --- |
| `aliyun` | ECS compute, VPC/network, SLB/public IP/DNS edge |
| `tencent` | CVM compute, VPC/network, CLB/public IP/DNS edge |

Each module may be `supported`, `partial`, or `unsupported`; a permission,
region, pagination, or rate-limit failure only changes that module's sync
coverage and never fabricates deletion of another module.

## Security and verification

Provider secrets are transient `SecretValue` values and may only enter a
transport request object. Logs, DTOs, coverage snapshots, and fixtures contain
no secret, response body, account identifier, host, or IP. Tests use fake
transports and synthetic `example.test` data. Live credentials, MCP acceptance,
Apple identities, and signing remain deferred by authorization.
