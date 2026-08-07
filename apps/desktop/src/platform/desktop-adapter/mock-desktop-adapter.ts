import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ConnectorCoverageSnapshotDto } from "../../generated/query/ConnectorCoverageSnapshotDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type { SyncStatusDto } from "../../generated/query/SyncStatusDto";

import type { DesktopAdapter } from "./desktop-adapter";
import type {
  GetResourceInput,
  GetTopologyInput,
  GitHubActionsSummarySnapshot,
  LocalSettings,
  QueryInvalidation,
  RecentChangesInput,
  RuntimeCapabilities,
  SearchResourcesInput,
  SyncStatusInput,
  TimelineInput,
  CreateBindingInput,
  CreateGitHubConnectionInput,
  DisableBindingInput,
  UpdateBindingInput,
  Unsubscribe,
} from "./desktop-adapter";

export interface DesktopAdapterSnapshot {
  readonly metadata: SnapshotMetadata | null;
  readonly resources: readonly ResourceDto[];
  readonly relations: readonly RelationDto[];
  readonly connections: readonly ConnectionDto[];
}

function copyMetadata(metadata: SnapshotMetadata | null): SnapshotMetadata | null {
  return metadata === null ? null : { ...metadata };
}

function copyItems<T extends object>(items: readonly T[]): T[] {
  return items.map((item) => ({ ...item }));
}

function copySnapshot(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    metadata: copyMetadata(snapshot.metadata),
    resources: copyItems(snapshot.resources),
    relations: copyItems(snapshot.relations),
    connections: copyItems(snapshot.connections),
  };
}

export class MockDesktopAdapter implements DesktopAdapter {
  readonly #snapshot: DesktopAdapterSnapshot;
  #settings: LocalSettings = {
    start_at_login: false,
    data_budget_mb: 512,
    retention_days: 30,
    user_quit: false,
  };

  constructor(snapshot: DesktopAdapterSnapshot) {
    this.#snapshot = copySnapshot(snapshot);
  }

  async listConnections() {
    return {
      metadata: this.#metadata(),
      items: copyItems(this.#snapshot.connections),
    };
  }

  async searchResources(_input: SearchResourcesInput = {}) {
    return {
      metadata: this.#metadata(),
      items: copyItems(this.#snapshot.resources),
      page_info: { next_cursor: null },
    };
  }

  async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    const resource = this.#snapshot.resources.find(
      (item) => item.resource_id === input.resource_id,
    );
    if (resource === undefined) throw new Error("Fixture resource was not found.");
    return {
      metadata: this.#metadata(),
      resource: { ...resource },
      attributes: {},
      relations: copyItems(
        this.#snapshot.relations.filter(
          (relation) =>
            relation.source_resource_id === input.resource_id ||
            relation.target_resource_id === input.resource_id,
        ),
      ),
      recent_changes: [],
      connector_coverage: [],
    };
  }

  async getTopology(input: GetTopologyInput) {
    if (
      !this.#snapshot.resources.some(
        (item) => item.resource_id === input.focus_resource_id,
      )
    ) {
      throw new Error("Fixture topology focus was not found.");
    }
    return {
      metadata: this.#metadata(),
      focus_resource_id: input.focus_resource_id,
      depth: input.depth ?? 1,
      nodes: copyItems(this.#snapshot.resources),
      edges: copyItems(this.#snapshot.relations),
      frontier: [],
      truncated: false,
    };
  }

  async getHealthSummary() {
    const resource_health = {
      healthy: 0,
      degraded: 0,
      unhealthy: 0,
      unknown: 0,
    };
    const freshness = { fresh: 0, stale: 0, expired: 0 };
    const connector_health = {
      healthy: 0,
      degraded: 0,
      auth_failed: 0,
      rate_limited: 0,
      unreachable: 0,
      disabled: 0,
    };
    for (const resource of this.#snapshot.resources) {
      resource_health[resource.health] += 1;
      freshness[resource.freshness] += 1;
    }
    for (const connection of this.#snapshot.connections) {
      connector_health[connection.health] += 1;
    }
    return {
      metadata: this.#metadata(),
      resource_health,
      freshness,
      connector_health,
    };
  }

  async getRecentChanges(_input: RecentChangesInput = {}) {
    return {
      metadata: this.#metadata(),
      items: [],
      page_info: { next_cursor: null },
    };
  }

  async getTimeline(_input: TimelineInput = {}) {
    return {
      metadata: this.#metadata(),
      groups: [],
      page_info: { next_cursor: null },
    };
  }

  async createBinding(input: CreateBindingInput) {
    return { metadata: this.#metadata(), binding: { binding_id: "fixture-binding", ...input, status: "active" as const, created_at: "2000-01-01T00:00:00Z", updated_at: "2000-01-01T00:00:00Z" } };
  }

  async updateBinding(input: UpdateBindingInput) {
    return { metadata: this.#metadata(), binding: { ...input, status: "active" as const, created_at: "2000-01-01T00:00:00Z", updated_at: "2000-01-01T00:00:01Z" } };
  }

  async disableBinding(input: DisableBindingInput) {
    return { metadata: this.#metadata(), binding: { binding_id: input.binding_id, source_resource_id: "fixture-source", target_resource_id: "fixture-target", kind: "fixture.depends_on", status: "disabled" as const, created_at: "2000-01-01T00:00:00Z", updated_at: "2000-01-01T00:00:01Z" } };
  }

  async getSyncStatus(input: SyncStatusInput): Promise<SyncStatusDto> {
    const connection = this.#snapshot.connections.find(
      (item) => item.connection_id === input.connection_id,
    );
    if (connection === undefined) throw new Error("Fixture connection was not found.");
    return {
      metadata: this.#metadata(),
      connection: { ...connection },
      recent_runs: [],
      next_scheduled_at: null,
    };
  }

  async listConnectorCoverage(): Promise<ConnectorCoverageSnapshotDto> {
    return { metadata: this.#metadata(), items: [] };
  }

  async getGitHubActionsSummary(): Promise<GitHubActionsSummarySnapshot> {
    return { items: [] };
  }

  async discoverGitHubRepositories() {
    return [
      { id: "fixture-github-repository-1", name: "fixture/first-repository" },
      { id: "fixture-github-repository-2", name: "fixture/second-repository" },
    ];
  }

  async createGitHubConnection(input: CreateGitHubConnectionInput) {
    return {
      connection_id: `fixture-github-${input.display_name || "connection"}`,
      sync_run_id: "fixture-github-sync",
    };
  }

  async previewGitHubConnectionPurge(connectionId: string) {
    const resourceIds = new Set(
      this.#snapshot.resources
        .filter((resource) => resource.connection_id === connectionId)
        .map((resource) => resource.resource_id),
    );
    return {
      resources: resourceIds.size,
      relations: this.#snapshot.relations.filter(
        (relation) =>
          resourceIds.has(relation.source_resource_id) ||
          resourceIds.has(relation.target_resource_id),
      ).length,
      resource_versions: 0,
      relation_versions: 0,
      changes: 0,
      bindings: 0,
      sync_runs: 0,
    };
  }

  async purgeGitHubConnection(connectionId: string) {
    return this.previewGitHubConnectionPurge(connectionId);
  }

  async manualSync(connectionId: string) {
    return { sync_run_id: `fixture-manual-${connectionId}` };
  }

  async getLocalSettings(): Promise<LocalSettings> {
    return { ...this.#settings };
  }

  async updateLocalSettings(settings: LocalSettings): Promise<LocalSettings> {
    this.#settings = { ...settings };
    return { ...this.#settings };
  }

  async getRuntimeCapabilities(): Promise<RuntimeCapabilities> {
    return {
      start_at_login: true,
      manual_sync: true,
      mcp_auto_launch: false,
      mcp_auto_launch_reason: "Trusted MCP integration is not installed, enabled, or verified for this App.",
    };
  }

  async subscribeInvalidations(
    _listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe> {
    return () => undefined;
  }

  #metadata(): SnapshotMetadata {
    if (this.#snapshot.metadata === null) {
      throw new Error("Fixture snapshot metadata is unavailable.");
    }
    return { ...this.#snapshot.metadata };
  }
}
