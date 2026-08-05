import type { ConnectionSnapshotDto } from "../../generated/query/ConnectionSnapshotDto";

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

export class EmptyDesktopAdapter implements DesktopAdapter {
  async listConnections(): Promise<ConnectionSnapshotDto> {
    throw new Error("Desktop query service is unavailable.");
  }

  async searchResources(_input?: SearchResourcesInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getResource(_input: GetResourceInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getTopology(_input: GetTopologyInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getHealthSummary(): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getRecentChanges(_input?: RecentChangesInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getSyncStatus(_input: SyncStatusInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async listConnectorCoverage(): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async manualSync(_connectionId: string): Promise<never> {
    throw new Error("Manual sync is unavailable.");
  }

  async getLocalSettings(): Promise<LocalSettings> {
    return {
      start_at_login: false,
      data_budget_mb: 0,
      retention_days: 0,
      user_quit: false,
    };
  }

  async updateLocalSettings(settings: LocalSettings): Promise<LocalSettings> {
    return { ...settings };
  }

  async getRuntimeCapabilities(): Promise<RuntimeCapabilities> {
    return {
      start_at_login: false,
      manual_sync: false,
      mcp_auto_launch: false,
      mcp_auto_launch_reason: "Desktop Host capabilities are unavailable.",
    };
  }

  async subscribeInvalidations(
    _listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe> {
    return () => undefined;
  }
}
