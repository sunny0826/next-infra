import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

import type { ChangePageDto } from "../../generated/query/ChangePageDto";
import type { ConnectorCoverageSnapshotDto } from "../../generated/query/ConnectorCoverageSnapshotDto";
import type { ErrorEnvelope } from "../../generated/query/ErrorEnvelope";
import type { HealthSummaryDto } from "../../generated/query/HealthSummaryDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourcePageDto } from "../../generated/query/ResourcePageDto";
import type { SyncStatusDto } from "../../generated/query/SyncStatusDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import type { TimelinePageDto } from "../../generated/query/TimelinePageDto";
import type { BindingCommandResultDto } from "../../generated/query/BindingCommandResultDto";

import type {
  DesktopAdapter,
  GetResourceInput,
  GetTopologyInput,
  LocalSettings,
  ManualSyncResult,
  QueryInvalidation,
  RecentChangesInput,
  RuntimeCapabilities,
  SearchResourcesInput,
  SyncStatusInput,
  TimelineInput,
  CreateBindingInput,
  CreateGitHubConnectionInput,
  UpdateBindingInput,
  DisableBindingInput,
  Unsubscribe,
} from "./desktop-adapter";

const INVALIDATION_EVENT = "next-infra://query-invalidated";

export interface DesktopTransport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(
    event: string,
    listener: (payload: T) => void,
  ): Promise<Unsubscribe>;
}

const TAURI_TRANSPORT: DesktopTransport = {
  invoke: (command, args) => tauriInvoke(command, args),
  listen: async (event, listener) =>
    tauriListen<TauriEventPayload<unknown>>(event, ({ payload }) => {
      listener(payload as never);
    }),
};

interface TauriEventPayload<T> {
  readonly payload: T;
}

function isErrorEnvelope(value: unknown): value is ErrorEnvelope {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<ErrorEnvelope>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.retryable === "boolean"
  );
}

export class DesktopAdapterError extends Error {
  readonly code: string;
  readonly retryable: boolean;

  constructor(code: string, message: string, retryable: boolean) {
    super(message);
    this.name = "DesktopAdapterError";
    this.code = code;
    this.retryable = retryable;
  }
}

export class RealDesktopAdapter implements DesktopAdapter {
  readonly #transport: DesktopTransport;

  constructor(transport: DesktopTransport = TAURI_TRANSPORT) {
    this.#transport = transport;
  }

  async #invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await this.#transport.invoke<T>(command, args);
    } catch (error) {
      if (isErrorEnvelope(error)) {
        throw new DesktopAdapterError(error.code, error.message, error.retryable);
      }
      throw new DesktopAdapterError(
        "desktop_transport_failed",
        "The local desktop service could not complete the request.",
        true,
      );
    }
  }

  async listConnections() {
    return this.#invoke<Awaited<ReturnType<DesktopAdapter["listConnections"]>>>(
      "query_list_connections",
    );
  }

  async searchResources(input: SearchResourcesInput = {}) {
    return this.#invoke<ResourcePageDto>("query_search_resources", { request: input });
  }

  async getResource(input: GetResourceInput) {
    return this.#invoke<ResourceDetailDto>("query_get_resource", { request: input });
  }

  async getTopology(input: GetTopologyInput) {
    return this.#invoke<TopologyDto>("query_get_topology", { request: input });
  }

  async getHealthSummary() {
    return this.#invoke<HealthSummaryDto>("query_health_summary");
  }

  async getRecentChanges(input: RecentChangesInput = {}) {
    return this.#invoke<ChangePageDto>("query_recent_changes", { request: input });
  }

  async getTimeline(input: TimelineInput = {}) {
    return this.#invoke<TimelinePageDto>("query_timeline", { request: input });
  }

  async createBinding(input: CreateBindingInput) {
    return this.#invoke<BindingCommandResultDto>("binding_create", { request: input });
  }

  async updateBinding(input: UpdateBindingInput) {
    return this.#invoke<BindingCommandResultDto>("binding_update", { request: input });
  }

  async disableBinding(input: DisableBindingInput) {
    return this.#invoke<BindingCommandResultDto>("binding_disable", { request: input });
  }

  async getSyncStatus(input: SyncStatusInput) {
    return this.#invoke<SyncStatusDto>("query_sync_status", { request: input });
  }

  async listConnectorCoverage() {
    return this.#invoke<ConnectorCoverageSnapshotDto>("query_connector_coverage");
  }

  async discoverGitHubRepositories(token: string) {
    return this.#invoke<Awaited<ReturnType<DesktopAdapter["discoverGitHubRepositories"]>>>(
      "github_discover_repositories",
      { request: { token } },
    );
  }

  async createGitHubConnection(input: CreateGitHubConnectionInput) {
    return this.#invoke<Awaited<ReturnType<DesktopAdapter["createGitHubConnection"]>>>(
      "github_connect",
      { request: input },
    );
  }

  async previewGitHubConnectionPurge(connectionId: string) {
    return this.#invoke<Awaited<ReturnType<DesktopAdapter["previewGitHubConnectionPurge"]>>>(
      "github_connection_purge_preview",
      { request: { connection_id: connectionId } },
    );
  }

  async purgeGitHubConnection(connectionId: string) {
    return this.#invoke<Awaited<ReturnType<DesktopAdapter["purgeGitHubConnection"]>>>(
      "github_connection_purge",
      { request: { connection_id: connectionId } },
    );
  }

  async manualSync(connectionId: string) {
    return this.#invoke<ManualSyncResult>("runtime_manual_sync", {
      connectionId,
    });
  }

  async getLocalSettings() {
    return this.#invoke<LocalSettings>("local_settings_get");
  }

  async updateLocalSettings(settings: LocalSettings) {
    return this.#invoke<LocalSettings>("local_settings_update", { settings });
  }

  async getRuntimeCapabilities() {
    return this.#invoke<RuntimeCapabilities>("runtime_capabilities");
  }

  async subscribeInvalidations(
    listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe> {
    return this.#transport.listen<QueryInvalidation>(INVALIDATION_EVENT, listener);
  }
}
