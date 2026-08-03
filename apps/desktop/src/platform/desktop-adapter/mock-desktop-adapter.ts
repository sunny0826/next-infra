import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";

import type { DesktopAdapter } from "./desktop-adapter";
import type {
  GetResourceInput,
  GetTopologyInput,
  LocalSettings,
  QueryInvalidation,
  RecentChangesInput,
  RuntimeCapabilities,
  SearchResourcesInput,
  SyncStatusInput,
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

  async getSnapshotMetadata(): Promise<SnapshotMetadata | null> {
    return copyMetadata(this.#snapshot.metadata);
  }

  async listResources(): Promise<readonly ResourceDto[]> {
    return copyItems(this.#snapshot.resources);
  }

  async listRelations(): Promise<readonly RelationDto[]> {
    return copyItems(this.#snapshot.relations);
  }

  async listConnections(): Promise<readonly ConnectionDto[]> {
    return copyItems(this.#snapshot.connections);
  }

  async searchResources(_input: SearchResourcesInput = {}) {
    return {
      metadata: this.#metadata(),
      items: copyItems(this.#snapshot.resources),
      page_info: { next_cursor: null },
    };
  }

  async getResource(input: GetResourceInput) {
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

  async getSyncStatus(input: SyncStatusInput) {
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

  async listConnectorCoverage() {
    return { metadata: this.#metadata(), items: [] };
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
