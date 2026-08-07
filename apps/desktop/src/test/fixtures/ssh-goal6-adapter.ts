import type { ConnectorCoverageDto } from "../../generated/query/ConnectorCoverageDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { SyncRunDto } from "../../generated/query/SyncRunDto";
import type {
  GetResourceInput,
  SearchResourcesInput,
  SyncStatusInput,
} from "../../platform/desktop-adapter/desktop-adapter";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";

const observedAt = "2000-01-01T00:00:00Z";
const unreachableConnection = "fixture-ssh-unreachable-connection";
const hostKeyConnection = "fixture-ssh-host-key-connection";

const snapshot = {
  metadata: {
    schema_version: 1,
    snapshot_version: "fixture-ssh-goal6-v1",
    generated_at: observedAt,
  },
  resources: [
    {
      resource_id: "fixture-ssh-host-unreachable",
      connection_id: unreachableConnection,
      kind: "ssh.host",
      display_name: "Fixture SSH Host Alpha",
      scope: "fixture-ssh-scope-alpha",
      lifecycle: "active" as const,
      health: "healthy" as const,
      freshness: "stale" as const,
      observed_at: "1999-12-31T23:55:00Z",
    },
    {
      resource_id: "fixture-ssh-filesystems-alpha",
      connection_id: unreachableConnection,
      kind: "ssh.filesystem",
      display_name: "Fixture SSH Filesystems Alpha",
      scope: "fixture-ssh-scope-alpha",
      lifecycle: "active" as const,
      health: "unknown" as const,
      freshness: "stale" as const,
      observed_at: "1999-12-31T23:55:00Z",
    },
    {
      resource_id: "fixture-ssh-host-key",
      connection_id: hostKeyConnection,
      kind: "ssh.host",
      display_name: "Fixture SSH Host Beta",
      scope: "fixture-ssh-scope-beta",
      lifecycle: "active" as const,
      health: "unknown" as const,
      freshness: "expired" as const,
      observed_at: "1999-12-31T23:00:00Z",
    },
  ],
  relations: [
    {
      relation_id: "fixture-ssh-host-filesystems",
      source_resource_id: "fixture-ssh-host-unreachable",
      target_resource_id: "fixture-ssh-filesystems-alpha",
      kind: "ssh.contains",
      lifecycle: "active" as const,
      evidence_type: "provider" as const,
      evidence: {
        type: "provider" as const,
        connector_type: "ssh",
        connection_id: unreachableConnection,
        sync_run_id: "fixture-ssh-last-success",
        field_path: "attributes.host_identity",
      },
      last_seen_at: "1999-12-31T23:55:00Z",
    },
  ],
  connections: [
    {
      connection_id: unreachableConnection,
      connector_type: "ssh",
      display_name: "Fixture SSH Connection Alpha",
      enabled: true,
      health: "unreachable" as const,
      last_success_at: "1999-12-31T23:55:00Z",
      last_attempt_at: observedAt,
    },
    {
      connection_id: hostKeyConnection,
      connector_type: "ssh",
      display_name: "Fixture SSH Connection Beta",
      enabled: true,
      health: "auth_failed" as const,
      last_success_at: "1999-12-31T23:00:00Z",
      last_attempt_at: observedAt,
    },
  ],
};

const coverage: readonly ConnectorCoverageDto[] = [
  "ssh.host",
  "ssh.filesystems",
  "ssh.process-summary",
  "ssh.launchd-services",
  "ssh.systemd-services",
].map((module) => ({
  connector_type: "ssh",
  connector_version: "1.0.0",
  module,
  level: "supported",
  reason: null,
}));

function failedRun(
  connectionId: string,
  syncRunId: string,
  code: string,
  message: string,
): SyncRunDto {
  return {
    sync_run_id: syncRunId,
    connection_id: connectionId,
    mode: "full",
    trigger: "schedule",
    status: "failed",
    coverage: {
      type: "partial",
      scope: connectionId,
      reason: "provider_unavailable",
    },
    started_at: observedAt,
    finished_at: observedAt,
    cursor_before: null,
    cursor_after: null,
    counts: { read: 0, created: 0, updated: 0, unchanged: 0, warnings: 1 },
    errors: [{ code, message, retryable: false }],
  };
}

const runs: Readonly<Record<string, SyncRunDto>> = {
  [unreachableConnection]: failedRun(
    unreachableConnection,
    "fixture-ssh-unreachable-run",
    "network_unreachable",
    "SSH host is unreachable.",
  ),
  [hostKeyConnection]: failedRun(
    hostKeyConnection,
    "fixture-ssh-host-key-run",
    "host_key_mismatch",
    "SSH host key verification failed. Trust was not changed.",
  ),
};

const attributes: Readonly<Record<string, Readonly<Record<string, unknown>>>> = {
  "fixture-ssh-host-unreachable": {
    platform: "darwin",
    architecture: "arm64",
    uptime_bucket: "7d_30d",
  },
  "fixture-ssh-filesystems-alpha": { entries: 2 },
  "fixture-ssh-host-key": {
    platform: "linux",
    architecture: "x86_64",
    uptime_bucket: "1d_7d",
  },
};

export class SshGoal6Adapter extends MockDesktopAdapter {
  constructor() {
    super(snapshot);
  }

  override async listConnectorCoverage() {
    return { metadata: snapshot.metadata, items: [...coverage] };
  }

  override async searchResources(input: SearchResourcesInput = {}) {
    const page = await super.searchResources(input);
    const query = input.query?.trim().toLocaleLowerCase("en");
    if (!query) return page;
    return {
      ...page,
      items: page.items.filter((resource) =>
        [resource.display_name, resource.kind].some((value) =>
          value.toLocaleLowerCase("en").includes(query),
        ),
      ),
    };
  }

  override async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    const detail = await super.getResource(input);
    return {
      ...detail,
      attributes: { ...(attributes[input.resource_id] ?? {}) },
      connector_coverage: [...coverage],
    };
  }

  override async getSyncStatus(input: SyncStatusInput) {
    const status = await super.getSyncStatus(input);
    const run = runs[input.connection_id];
    if (run === undefined) throw new Error("Fixture sync run was not found.");
    return { ...status, recent_runs: [run] };
  }
}

export function createSshGoal6Adapter(): SshGoal6Adapter {
  return new SshGoal6Adapter();
}
