import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { SyncRunDto } from "../../generated/query/SyncRunDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";

import type { DesktopAdapterSnapshot } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { FIXTURE_OBSERVED_AT } from "./query-fixtures";

/**
 * Synthetic SyncRun provenance fixture.
 *
 * Values intentionally do not identify a real host, repository, address, or
 * credential. Every id uses the `fixture-` prefix and every timestamp is
 * fixed; sync runs are ordered most-recent-first to mirror the query contract.
 * The snapshot carries `sync_runs` only as an opt-in extension — adapters
 * built without it keep the default empty-run behavior.
 */

const FIXTURE_SYNC_CONNECTION_ID = "fixture-connection-sync";
const FIXTURE_NEVER_SYNCED_CONNECTION_ID = "fixture-connection-never";

function connection(
  connectionId: string,
  displayName: string,
  lastSuccessAt: string | null,
  lastAttemptAt: string | null,
): ConnectionDto {
  return {
    connection_id: connectionId,
    connector_type: "fixture",
    display_name: displayName,
    enabled: true,
    health: "healthy",
    last_success_at: lastSuccessAt,
    last_attempt_at: lastAttemptAt,
  };
}

function syncRun(
  syncRunId: string,
  startedAt: string,
  finishedAt: string | null,
  run: Pick<SyncRunDto, "mode" | "trigger" | "status" | "coverage" | "counts"> &
    Partial<Pick<SyncRunDto, "errors" | "warnings">>,
): SyncRunDto {
  return {
    sync_run_id: syncRunId,
    connection_id: FIXTURE_SYNC_CONNECTION_ID,
    started_at: startedAt,
    finished_at: finishedAt,
    cursor_before: null,
    cursor_after: null,
    errors: [],
    warnings: [],
    ...run,
  };
}

/**
 * Snapshot carrying five runs on one connection (most recent first) and one
 * connection that has never produced a SyncRun. Covers: succeeded +
 * authoritative_full, incremental coverage, running, partial with a coverage
 * reason, and failed with an error, plus the never-synced empty state.
 */
export function createSyncRunSnapshotFixture(): DesktopAdapterSnapshot {
  return {
    metadata: metadata("fixture-sync-run-snapshot-v1"),
    resources: [],
    relations: [],
    connections: [
      connection(
        FIXTURE_SYNC_CONNECTION_ID,
        "Fixture Sync Connection",
        "2000-01-05T00:00:15Z",
        "2000-01-06T00:00:09Z",
      ),
      connection(
        FIXTURE_NEVER_SYNCED_CONNECTION_ID,
        "Fixture Never-Synced Connection",
        null,
        null,
      ),
    ],
    sync_runs: [
      syncRun("fixture-sync-run-failed", "2000-01-06T00:00:00Z", "2000-01-06T00:00:09Z", {
        mode: "incremental",
        trigger: "recovery",
        status: "failed",
        coverage: { type: "partial", scope: null, reason: "Fixture: run aborted before coverage was completed." },
        counts: { read: 3, created: 0, updated: 0, unchanged: 0, warnings: 0 },
        errors: [{ code: "fixture_auth_expired", message: "Fixture: credential refresh failed.", retryable: true }],
      }),
      syncRun("fixture-sync-run-partial", "2000-01-05T00:00:00Z", "2000-01-05T00:00:15Z", {
        mode: "incremental",
        trigger: "user",
        status: "partial",
        coverage: { type: "partial", scope: "fixture-scope", reason: "Fixture: remaining pages skipped after quota limit." },
        counts: { read: 8, created: 0, updated: 1, unchanged: 7, warnings: 2 },
        warnings: [{ code: "fixture_quota_exceeded", message: "Fixture: quota limited further reads." }],
      }),
      syncRun("fixture-sync-run-incremental", "2000-01-04T00:00:00Z", "2000-01-04T00:00:21Z", {
        mode: "incremental",
        trigger: "schedule",
        status: "succeeded",
        coverage: { type: "incremental", cursor: "fixture-cursor-v2" },
        counts: { read: 12, created: 1, updated: 2, unchanged: 9, warnings: 1 },
        warnings: [{ code: "fixture_rate_limit_proximity", message: "Fixture: rate limit headroom below threshold." }],
      }),
      syncRun("fixture-sync-run-running", "2000-01-03T00:00:00Z", null, {
        mode: "full",
        trigger: "startup",
        status: "running",
        coverage: { type: "authoritative_full", scope: "fixture-scope" },
        counts: { read: 0, created: 0, updated: 0, unchanged: 0, warnings: 0 },
      }),
      syncRun("fixture-sync-run-full", "2000-01-02T00:00:00Z", "2000-01-02T00:00:42Z", {
        mode: "full",
        trigger: "schedule",
        status: "succeeded",
        coverage: { type: "authoritative_full", scope: "fixture-scope" },
        counts: { read: 42, created: 3, updated: 5, unchanged: 34, warnings: 0 },
      }),
    ],
  };
}

function metadata(snapshotVersion: string): SnapshotMetadata {
  return {
    schema_version: 1,
    snapshot_version: snapshotVersion,
    generated_at: FIXTURE_OBSERVED_AT,
  };
}
