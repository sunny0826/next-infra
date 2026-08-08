import type { ConnectionSnapshotDto } from "../../generated/query/ConnectionSnapshotDto";
import type { ChangePageDto } from "../../generated/query/ChangePageDto";
import type { ConnectorCoverageSnapshotDto } from "../../generated/query/ConnectorCoverageSnapshotDto";
import type { Freshness } from "../../generated/query/Freshness";
import type { HealthSummaryDto } from "../../generated/query/HealthSummaryDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceHealth } from "../../generated/query/ResourceHealth";
import type { ResourcePageDto } from "../../generated/query/ResourcePageDto";
import type { SyncStatusDto } from "../../generated/query/SyncStatusDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import type { TimelinePageDto } from "../../generated/query/TimelinePageDto";
import type { BindingCommandResultDto } from "../../generated/query/BindingCommandResultDto";

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

export interface TimelineInput {
  readonly limit?: number;
  readonly cursor?: string;
}

export interface CreateBindingInput {
  readonly source_resource_id: string;
  readonly target_resource_id: string;
  readonly kind: string;
}

export interface UpdateBindingInput extends CreateBindingInput {
  readonly binding_id: string;
}

export interface DisableBindingInput {
  readonly binding_id: string;
}

export interface ManualSyncResult {
  readonly sync_run_id: string;
}

export interface CreateGitHubConnectionInput {
  readonly display_name: string;
  readonly token: string;
  readonly selected_repository_ids: readonly string[];
}

export interface GitHubRepositoryOption {
  readonly id: string;
  readonly name: string;
}

export interface CreateGitHubConnectionResult {
  readonly connection_id: string;
  readonly sync_run_id: string;
}

export interface SshValidateInput {
  readonly host_alias: string;
  readonly connect_timeout_secs?: number;
}

export interface SshServiceOption {
  readonly id: string;
  readonly name: string;
}

export interface SshValidateResult {
  readonly discovered_services: readonly SshServiceOption[];
}

export interface SshConnectInput {
  readonly display_name: string;
  readonly host_alias: string;
  readonly connect_timeout_secs?: number;
  readonly allowed_service_ids: readonly string[];
}

export interface SshConnectResult {
  readonly connection_id: string;
  readonly sync_run_id: string;
}

export interface ConnectionPurgeSummary {
  readonly resources: number;
  readonly relations: number;
  readonly resource_versions: number;
  readonly relation_versions: number;
  readonly changes: number;
  readonly bindings: number;
  readonly sync_runs: number;
}

export interface GitHubRepositoryActions {
  readonly repository_id: string;
  readonly repository_name: string;
  readonly action_count: number;
  readonly succeeded: number;
  readonly failed: number;
  readonly running: number;
}

export interface GitHubActionsSummary {
  readonly connection_id: string;
  readonly connection_name: string;
  readonly repositories: readonly GitHubRepositoryActions[];
}

export interface GitHubActionsSummarySnapshot {
  readonly items: readonly GitHubActionsSummary[];
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
  readonly mcp_auto_launch_reason: string;
}

export interface QueryInvalidation {
  readonly version: string;
  readonly scopes: readonly string[];
}

export type Unsubscribe = () => void;

export function desktopErrorCode(error: unknown): string {
  return typeof error === "object" && error !== null && "code" in error && typeof error.code === "string"
    ? error.code
    : "desktop_transport_failed";
}

export interface DesktopAdapter {
  listConnections(): Promise<ConnectionSnapshotDto>;
  searchResources(input?: SearchResourcesInput): Promise<ResourcePageDto>;
  getResource(input: GetResourceInput): Promise<ResourceDetailDto>;
  getTopology(input: GetTopologyInput): Promise<TopologyDto>;
  getHealthSummary(): Promise<HealthSummaryDto>;
  getRecentChanges(input?: RecentChangesInput): Promise<ChangePageDto>;
  getTimeline(input?: TimelineInput): Promise<TimelinePageDto>;
  createBinding(input: CreateBindingInput): Promise<BindingCommandResultDto>;
  updateBinding(input: UpdateBindingInput): Promise<BindingCommandResultDto>;
  disableBinding(input: DisableBindingInput): Promise<BindingCommandResultDto>;
  getSyncStatus(input: SyncStatusInput): Promise<SyncStatusDto>;
  listConnectorCoverage(): Promise<ConnectorCoverageSnapshotDto>;
  getGitHubActionsSummary(): Promise<GitHubActionsSummarySnapshot>;
  discoverGitHubRepositories(token: string): Promise<readonly GitHubRepositoryOption[]>;
  createGitHubConnection(input: CreateGitHubConnectionInput): Promise<CreateGitHubConnectionResult>;
  validateSshConnection(input: SshValidateInput): Promise<SshValidateResult>;
  createSshConnection(input: SshConnectInput): Promise<SshConnectResult>;
  previewGitHubConnectionPurge(connectionId: string): Promise<ConnectionPurgeSummary>;
  purgeGitHubConnection(connectionId: string): Promise<ConnectionPurgeSummary>;
  manualSync(connectionId: string): Promise<ManualSyncResult>;
  getLocalSettings(): Promise<LocalSettings>;
  updateLocalSettings(settings: LocalSettings): Promise<LocalSettings>;
  getRuntimeCapabilities(): Promise<RuntimeCapabilities>;
  subscribeInvalidations(
    listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe>;
}
