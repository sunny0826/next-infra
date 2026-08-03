import { describe, expect, it } from "vitest";

import {
  createConnectorCoverageFixtures,
  createEmptyQuerySnapshotFixture,
  createQueryChangeFixture,
  createQueryErrorEnvelopeFixture,
  createQueryEvidenceLifecycleSnapshotFixture,
  createQueryPageInfoFixture,
  createQuerySnapshotMetadataFixture,
  createQueryViewStateFixtures,
  createSyncRunFixtures,
  createUnresolvedRelationSnapshotFixture,
} from "./query-fixtures";

describe("UI query fixture catalog", () => {
  it("covers the QDTO-supported lifecycle, freshness, health, and evidence variants", () => {
    const fixture = createQueryEvidenceLifecycleSnapshotFixture();

    expect(fixture.resources.map(({ lifecycle }) => lifecycle)).toEqual([
      "active",
      "active",
      "tombstoned",
      "orphaned",
    ]);
    expect(fixture.resources).toContainEqual(
      expect.objectContaining({ health: "healthy", freshness: "expired" }),
    );
    expect(fixture.relations.map(({ evidence_type }) => evidence_type)).toEqual([
      "provider",
      "configured",
      "inferred",
    ]);
    expect(
      new Set(
        fixture.relations.map(({ source_resource_id, target_resource_id }) =>
          `${source_resource_id}->${target_resource_id}`,
        ),
      ),
    ).toEqual(new Set(["fixture-resource-alpha->fixture-resource-beta"]));
    expect(fixture.connections.map(({ health }) => health)).toEqual([
      "healthy",
      "unreachable",
      "disabled",
    ]);
  });

  it("serializes to the same bytes for every fresh fixture instance", () => {
    expect(JSON.stringify(createQueryEvidenceLifecycleSnapshotFixture())).toBe(
      JSON.stringify(createQueryEvidenceLifecycleSnapshotFixture()),
    );
    expect(JSON.stringify(createEmptyQuerySnapshotFixture())).toBe(
      JSON.stringify(createEmptyQuerySnapshotFixture()),
    );
    expect(JSON.stringify(createQueryErrorEnvelopeFixture())).toBe(
      JSON.stringify(createQueryErrorEnvelopeFixture()),
    );
    expect(JSON.stringify(createQueryPageInfoFixture())).toBe(
      JSON.stringify(createQueryPageInfoFixture()),
    );
    expect(JSON.stringify(createQuerySnapshotMetadataFixture())).toBe(
      JSON.stringify(createQuerySnapshotMetadataFixture()),
    );
    expect(JSON.stringify(createQueryChangeFixture())).toBe(
      JSON.stringify(createQueryChangeFixture()),
    );
    expect(JSON.stringify(createConnectorCoverageFixtures())).toBe(
      JSON.stringify(createConnectorCoverageFixtures()),
    );
    expect(JSON.stringify(createSyncRunFixtures())).toBe(
      JSON.stringify(createSyncRunFixtures()),
    );
    expect(JSON.stringify(createQueryViewStateFixtures())).toBe(
      JSON.stringify(createQueryViewStateFixtures()),
    );
    expect(JSON.stringify(createUnresolvedRelationSnapshotFixture())).toBe(
      JSON.stringify(createUnresolvedRelationSnapshotFixture()),
    );
  });

  it("contains only synthetic identifiers and no sensitive or real infrastructure values", () => {
    const serialized = JSON.stringify({
      snapshot: createQueryEvidenceLifecycleSnapshotFixture(),
      empty: createEmptyQuerySnapshotFixture(),
      error: createQueryErrorEnvelopeFixture(),
      page: createQueryPageInfoFixture(),
      change: createQueryChangeFixture(),
      coverage: createConnectorCoverageFixtures(),
      syncRuns: createSyncRunFixtures(),
      states: createQueryViewStateFixtures(),
      unresolved: createUnresolvedRelationSnapshotFixture(),
    });

    expect(serialized).toContain("fixture-");
    expect(serialized).not.toMatch(/github\.com|10\.0\.|192\.168\.|https?:\/\//);
    expect(serialized).not.toMatch(/secret|password|token/i);
  });

  it("keeps empty state distinct from a committed snapshot", () => {
    const fixture = createEmptyQuerySnapshotFixture();

    expect(fixture.metadata).toBeNull();
    expect(fixture.resources).toEqual([]);
    expect(fixture.relations).toEqual([]);
    expect(fixture.connections).toEqual([]);
  });

  it("covers change, connector coverage, sync coverage, and view-state contracts", () => {
    const syncRuns = createSyncRunFixtures();

    expect(createQueryViewStateFixtures()).toEqual([
      "loading",
      "ready",
      "empty",
      "partial",
      "error",
    ]);
    expect(createConnectorCoverageFixtures().map(({ level }) => level)).toEqual([
      "supported",
      "partial",
      "unsupported",
    ]);
    expect(syncRuns.map(({ coverage }) => coverage.type)).toEqual([
      "authoritative_full",
      "incremental",
      "targeted",
      "partial",
      "authoritative_full",
      "authoritative_full",
    ]);
    expect(syncRuns.map(({ status }) => status)).toContain("failed");
    expect(syncRuns.map(({ status }) => status)).toContain("interrupted");
    expect(createQueryChangeFixture()).toEqual(
      expect.objectContaining({
        subject: { type: "resource", resource_id: "fixture-resource-alpha" },
        origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
      }),
    );
  });

  it("represents unresolved relations without inventing endpoint resources", () => {
    const fixture = createUnresolvedRelationSnapshotFixture();
    const targetId = fixture.relations[0].target_resource_id;

    expect(targetId).toBe("fixture-resource-unresolved");
    expect(fixture.resources.some(({ resource_id }) => resource_id === targetId)).toBe(false);
  });
});
