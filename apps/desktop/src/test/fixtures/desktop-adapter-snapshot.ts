import type { DesktopAdapterSnapshot } from "../../platform/desktop-adapter/mock-desktop-adapter";

export function createDesktopAdapterSnapshotFixture(): DesktopAdapterSnapshot {
  return {
    metadata: {
      schema_version: 1,
      snapshot_version: "fixture-snapshot-v1",
      generated_at: "2000-01-01T00:00:00Z",
    },
    resources: [
      {
        resource_id: "fixture-resource-alpha",
        connection_id: "fixture-connection-alpha",
        kind: "fixture.compute.node",
        display_name: "Fixture Compute Alpha",
        scope: "fixture-scope",
        lifecycle: "active",
        health: "healthy",
        freshness: "fresh",
        observed_at: "2000-01-01T00:00:00Z",
      },
      {
        resource_id: "fixture-resource-beta",
        connection_id: "fixture-connection-alpha",
        kind: "fixture.database.instance",
        display_name: "Fixture Database Beta",
        scope: "fixture-scope",
        lifecycle: "active",
        health: "unknown",
        freshness: "stale",
        observed_at: "2000-01-01T00:00:00Z",
      },
    ],
    relations: [
      {
        relation_id: "fixture-relation-alpha-beta",
        source_resource_id: "fixture-resource-alpha",
        target_resource_id: "fixture-resource-beta",
        kind: "depends_on",
        lifecycle: "active",
        evidence_type: "provider",
        evidence: {
          type: "provider",
          connector_type: "fixture",
          connection_id: "fixture-connection-alpha",
          sync_run_id: "fixture-sync-run-alpha",
          field_path: "attributes.target",
        },
        last_seen_at: "2000-01-01T00:00:00Z",
      },
    ],
    connections: [
      {
        connection_id: "fixture-connection-alpha",
        connector_type: "fixture",
        display_name: "Fixture Connection Alpha",
        enabled: true,
        health: "healthy",
        last_success_at: "2000-01-01T00:00:00Z",
        last_attempt_at: "2000-01-01T00:00:00Z",
      },
    ],
  };
}
