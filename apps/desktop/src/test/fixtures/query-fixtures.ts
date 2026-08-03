import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ChangeDto } from "../../generated/query/ChangeDto";
import type { ConnectorCoverageDto } from "../../generated/query/ConnectorCoverageDto";
import type { ErrorEnvelope } from "../../generated/query/ErrorEnvelope";
import type { PageInfo } from "../../generated/query/PageInfo";
import type { QueryViewState } from "../../generated/query/QueryViewState";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type { SyncRunDto } from "../../generated/query/SyncRunDto";

import type { DesktopAdapterSnapshot } from "../../platform/desktop-adapter/mock-desktop-adapter";

/**
 * Synthetic values shared by every UI query fixture.
 *
 * These values intentionally do not identify a real host, repository, address,
 * or credential. The fixture module only composes generated QDTOs; it does not
 * add a second query contract for states that QDTO cannot represent.
 */
export const FIXTURE_OBSERVED_AT = "2000-01-01T00:00:00Z";

const FIXTURE_SCHEMA_VERSION = 1;

function metadata(snapshotVersion: string): SnapshotMetadata {
  return {
    schema_version: FIXTURE_SCHEMA_VERSION,
    snapshot_version: snapshotVersion,
    generated_at: FIXTURE_OBSERVED_AT,
  };
}

/** Returns the committed-snapshot metadata used by the catalog. */
export function createQuerySnapshotMetadataFixture(): SnapshotMetadata {
  return metadata("fixture-query-snapshot-v1");
}

function resource(
  resourceId: string,
  connectionId: string,
  kind: string,
  displayName: string,
  lifecycle: ResourceDto["lifecycle"],
  health: ResourceDto["health"],
  freshness: ResourceDto["freshness"],
): ResourceDto {
  return {
    resource_id: resourceId,
    connection_id: connectionId,
    kind,
    display_name: displayName,
    lifecycle,
    health,
    freshness,
    observed_at: FIXTURE_OBSERVED_AT,
  };
}

function relation(
  relationId: string,
  evidenceType: RelationDto["evidence_type"],
  sourceResourceId = "fixture-resource-alpha",
  targetResourceId = "fixture-resource-beta",
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind: "fixture.depends_on",
    evidence_type: evidenceType,
    last_seen_at: FIXTURE_OBSERVED_AT,
  };
}

function connection(
  connectionId: string,
  displayName: string,
  enabled: boolean,
  health: ConnectionDto["health"],
  lastSuccessAt: string | null,
  lastAttemptAt: string | null,
): ConnectionDto {
  return {
    connection_id: connectionId,
    connector_type: "fixture",
    display_name: displayName,
    enabled,
    health,
    last_success_at: lastSuccessAt,
    last_attempt_at: lastAttemptAt,
  };
}

/**
 * Covers every Resource/Relation/Connection enum that the current QDTO can
 * carry in one deterministic snapshot.
 *
 * The three relations intentionally share endpoints while retaining distinct
 * IDs. This preserves provider/configured/inferred evidence as separate rows
 * without inventing provenance fields that are not present in RelationDto.
 */
export function createQueryEvidenceLifecycleSnapshotFixture(): DesktopAdapterSnapshot {
  return {
    metadata: metadata("fixture-query-evidence-lifecycle-v1"),
    resources: [
      resource(
        "fixture-resource-alpha",
        "fixture-connection-alpha",
        "fixture.compute.node",
        "Fixture Compute Alpha",
        "active",
        "healthy",
        "fresh",
      ),
      resource(
        "fixture-resource-beta",
        "fixture-connection-alpha",
        "fixture.database.instance",
        "Fixture Database Beta",
        "active",
        "healthy",
        "expired",
      ),
      resource(
        "fixture-resource-tombstoned",
        "fixture-connection-beta",
        "fixture.service.endpoint",
        "Fixture Tombstoned Endpoint",
        "tombstoned",
        "unknown",
        "expired",
      ),
      resource(
        "fixture-resource-orphaned",
        "fixture-connection-beta",
        "fixture.worker.process",
        "Fixture Orphaned Worker",
        "orphaned",
        "degraded",
        "stale",
      ),
    ],
    relations: [
      relation("fixture-relation-provider-alpha-beta", "provider"),
      relation("fixture-relation-configured-alpha-beta", "configured"),
      relation("fixture-relation-inferred-alpha-beta", "inferred"),
    ],
    connections: [
      connection(
        "fixture-connection-alpha",
        "Fixture Connection Alpha",
        true,
        "healthy",
        FIXTURE_OBSERVED_AT,
        FIXTURE_OBSERVED_AT,
      ),
      connection(
        "fixture-connection-beta",
        "Fixture Connection Beta",
        true,
        "unreachable",
        "1999-12-31T23:59:00Z",
        FIXTURE_OBSERVED_AT,
      ),
      connection(
        "fixture-connection-disabled",
        "Fixture Disabled Connection",
        false,
        "disabled",
        null,
        null,
      ),
    ],
  };
}

/**
 * Represents an uncommitted/empty adapter result using only the existing
 * DesktopAdapterSnapshot shape. A loading state needs a query status DTO and
 * is therefore deliberately not represented here.
 */
export function createEmptyQuerySnapshotFixture(): DesktopAdapterSnapshot {
  return {
    metadata: null,
    resources: [],
    relations: [],
    connections: [],
  };
}

/**
 * ErrorEnvelope is a generated QDTO, but DesktopAdapter has no error method;
 * callers can use this value in a query/error test without embedding a custom
 * error field in DesktopAdapterSnapshot.
 */
export function createQueryErrorEnvelopeFixture(): ErrorEnvelope {
  return {
    schema_version: FIXTURE_SCHEMA_VERSION,
    code: "fixture_permission_denied",
    message: "Fixture query access was denied.",
    retryable: false,
  };
}

/**
 * PageInfo uses an opaque cursor in the generated binding. The cast is limited
 * to this synthetic fixture value and does not weaken the production type.
 */
export function createQueryPageInfoFixture(): PageInfo {
  return {
    next_cursor: "fixture-cursor-v1" as PageInfo["next_cursor"],
  };
}

/** Every UI request state is explicit; empty and partial never masquerade as errors. */
export function createQueryViewStateFixtures(): readonly QueryViewState[] {
  return ["loading", "ready", "empty", "partial", "error"];
}

export function createQueryChangeFixture(): ChangeDto {
  return {
    change_id: "fixture-change-alpha",
    subject: { type: "resource", resource_id: "fixture-resource-alpha" },
    observed_at: FIXTURE_OBSERVED_AT,
    fields: [
      {
        path: "attributes.state",
        before: "pending",
        after: "ready",
      },
    ],
    origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
  };
}

export function createConnectorCoverageFixtures(): readonly ConnectorCoverageDto[] {
  return [
    {
      connector_type: "fixture",
      connector_version: "1.0.0",
      module: "fixture.compute",
      level: "supported",
      reason: null,
    },
    {
      connector_type: "fixture",
      connector_version: "1.0.0",
      module: "fixture.database",
      level: "partial",
      reason: "Fixture omits provider-specific maintenance details.",
    },
    {
      connector_type: "fixture",
      connector_version: "1.0.0",
      module: "fixture.billing",
      level: "unsupported",
      reason: "Fixture connector does not expose billing resources.",
    },
  ];
}

function syncRun(
  syncRunId: string,
  overrides: Partial<SyncRunDto>,
): SyncRunDto {
  return {
    sync_run_id: syncRunId,
    connection_id: "fixture-connection-alpha",
    mode: "full",
    trigger: "schedule",
    status: "succeeded",
    coverage: { type: "authoritative_full", scope: "fixture-scope" },
    started_at: FIXTURE_OBSERVED_AT,
    finished_at: "2000-01-01T00:00:01Z",
    cursor_before: null,
    cursor_after: "fixture-cursor-complete",
    counts: {
      read: 2,
      created: 2,
      updated: 0,
      unchanged: 0,
      warnings: 0,
    },
    errors: [],
    ...overrides,
  };
}

/** Covers every SyncCoverage branch plus failed/interrupted history states. */
export function createSyncRunFixtures(): readonly SyncRunDto[] {
  return [
    syncRun("fixture-sync-run-complete", {}),
    syncRun("fixture-sync-run-incremental", {
      mode: "incremental",
      coverage: { type: "incremental", cursor: "fixture-cursor-complete" },
      cursor_before: "fixture-cursor-complete",
      cursor_after: "fixture-cursor-incremental",
    }),
    syncRun("fixture-sync-run-targeted", {
      mode: "targeted",
      trigger: "user",
      coverage: {
        type: "targeted",
        resource_ids: ["fixture-resource-alpha"],
      },
      cursor_before: "fixture-cursor-incremental",
      cursor_after: "fixture-cursor-incremental",
    }),
    syncRun("fixture-sync-run-partial", {
      status: "partial",
      coverage: {
        type: "partial",
        scope: "fixture-scope",
        reason: "rate_limited",
      },
      cursor_before: "fixture-cursor-incremental",
      cursor_after: "fixture-cursor-recovery",
      counts: {
        read: 1,
        created: 0,
        updated: 0,
        unchanged: 1,
        warnings: 1,
      },
      errors: [
        {
          code: "rate_limited",
          message: "Fixture connector reached its deterministic limit.",
          retryable: true,
        },
      ],
    }),
    syncRun("fixture-sync-run-failed", {
      status: "failed",
      trigger: "recovery",
      cursor_before: "fixture-cursor-recovery",
      cursor_after: "fixture-cursor-recovery",
      errors: [
        {
          code: "provider_unavailable",
          message: "Fixture provider is unavailable.",
          retryable: true,
        },
      ],
    }),
    syncRun("fixture-sync-run-interrupted", {
      status: "interrupted",
      trigger: "startup",
      finished_at: "2000-01-01T00:00:02Z",
      cursor_before: "fixture-cursor-recovery",
      cursor_after: null,
    }),
  ];
}

/** A relation can remain visible while one endpoint is unresolved. */
export function createUnresolvedRelationSnapshotFixture(): DesktopAdapterSnapshot {
  const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
  return {
    ...snapshot,
    metadata: metadata("fixture-query-unresolved-v1"),
    relations: [
      relation(
        "fixture-relation-unresolved",
        "configured",
        "fixture-resource-alpha",
        "fixture-resource-unresolved",
      ),
    ],
  };
}
