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
  TimelineInput,
  CreateBindingInput,
  CreateGitHubConnectionInput,
  SshConnectInput,
  SshValidateInput,
  DisableBindingInput,
  UpdateBindingInput,
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

  async getTimeline(_input?: TimelineInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async createBinding(_input: CreateBindingInput): Promise<never> {
    throw new Error("Binding is unavailable.");
  }

  async updateBinding(_input: UpdateBindingInput): Promise<never> {
    throw new Error("Binding is unavailable.");
  }

  async disableBinding(_input: DisableBindingInput): Promise<never> {
    throw new Error("Binding is unavailable.");
  }

  async getSyncStatus(_input: SyncStatusInput): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async listConnectorCoverage(): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async getGitHubActionsSummary(): Promise<never> {
    throw new Error("Desktop query service is unavailable.");
  }

  async discoverGitHubRepositories(_token: string): Promise<never> {
    throw new Error("GitHub repository discovery is unavailable.");
  }

  async createGitHubConnection(_input: CreateGitHubConnectionInput): Promise<never> {
    throw new Error("GitHub connection creation is unavailable.");
  }

  async validateSshConnection(_input: SshValidateInput): Promise<never> {
    throw new Error("SSH connection validation is unavailable.");
  }

  async createSshConnection(_input: SshConnectInput): Promise<never> {
    throw new Error("SSH connection creation is unavailable.");
  }

  async previewGitHubConnectionPurge(_connectionId: string): Promise<never> {
    throw new Error("GitHub connection cleanup is unavailable.");
  }

  async purgeGitHubConnection(_connectionId: string): Promise<never> {
    throw new Error("GitHub connection cleanup is unavailable.");
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
