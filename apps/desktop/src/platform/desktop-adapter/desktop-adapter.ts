import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ChangePageDto } from "../../generated/query/ChangePageDto";
import type { ConnectorCoverageSnapshotDto } from "../../generated/query/ConnectorCoverageSnapshotDto";
import type { Freshness } from "../../generated/query/Freshness";
import type { HealthSummaryDto } from "../../generated/query/HealthSummaryDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { ResourceHealth } from "../../generated/query/ResourceHealth";
import type { ResourcePageDto } from "../../generated/query/ResourcePageDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type { SyncStatusDto } from "../../generated/query/SyncStatusDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";

export interface SearchResourcesInput {
  readonly query?: string;
  readonly kinds?: readonly string[];
  readonly connector_types?: readonly string[];
  readonly health?: readonly ResourceHealth[];
  readonly freshness?: readonly Freshness[];
  readonly labels?: Readonly<Record<string, string>>;
  readonly limit?: number;
  readonly cursor?: string;
}

export interface GetResourceInput {
  readonly resource_id: string;
  readonly include?: readonly (
    | "attributes"
    | "relations"
    | "recent_changes"
    | "connector_coverage"
  )[];
}

export interface GetTopologyInput {
  readonly focus_resource_id: string;
  readonly depth?: number;
  readonly max_nodes?: number;
  readonly max_edges?: number;
}

export interface RecentChangesInput {
  readonly since?: string;
  readonly resource_id?: string;
  readonly kinds?: readonly string[];
  readonly limit?: number;
  readonly cursor?: string;
}

export interface SyncStatusInput {
  readonly connection_id: string;
  readonly recent_run_limit?: number;
}

export interface ManualSyncResult {
  readonly sync_run_id: string;
}

export interface LocalSettings {
  readonly start_at_login: boolean;
  readonly data_budget_mb: number;
  readonly retention_days: number;
  readonly user_quit: boolean;
}

export interface RuntimeCapabilities {
  readonly start_at_login: boolean;
  readonly manual_sync: boolean;
  readonly mcp_auto_launch: boolean;
}

export interface QueryInvalidation {
  readonly version: string;
  readonly scopes: readonly string[];
}

export type Unsubscribe = () => void;

export interface DesktopAdapter {
  getSnapshotMetadata(): Promise<SnapshotMetadata | null>;
  listResources(): Promise<readonly ResourceDto[]>;
  listRelations(): Promise<readonly RelationDto[]>;
  listConnections(): Promise<readonly ConnectionDto[]>;
  searchResources(input?: SearchResourcesInput): Promise<ResourcePageDto>;
  getResource(input: GetResourceInput): Promise<ResourceDetailDto>;
  getTopology(input: GetTopologyInput): Promise<TopologyDto>;
  getHealthSummary(): Promise<HealthSummaryDto>;
  getRecentChanges(input?: RecentChangesInput): Promise<ChangePageDto>;
  getSyncStatus(input: SyncStatusInput): Promise<SyncStatusDto>;
  listConnectorCoverage(): Promise<ConnectorCoverageSnapshotDto>;
  manualSync(connectionId: string): Promise<ManualSyncResult>;
  getLocalSettings(): Promise<LocalSettings>;
  updateLocalSettings(settings: LocalSettings): Promise<LocalSettings>;
  getRuntimeCapabilities(): Promise<RuntimeCapabilities>;
  subscribeInvalidations(
    listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe>;
}
