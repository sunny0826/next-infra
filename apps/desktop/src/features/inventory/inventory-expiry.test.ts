import { describe, expect, it } from "vitest";

import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import {
  isExpired,
  isRemoved,
  matchesStatusFilter,
  needsAttention,
  summarizeConnectionExpiry,
} from "./inventory-expiry";

const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
const resources = snapshot.resources;

function resourceByDisplayName(displayName: string) {
  const resource = resources.find((item) => item.display_name === displayName);
  if (resource === undefined) throw new Error(`Fixture resource ${displayName} is missing.`);
  return resource;
}

describe("needsAttention", () => {
  it("matches the page's original semantics: any dimension differing from healthy/fresh/active", () => {
    expect(needsAttention(resourceByDisplayName("Fixture Compute Alpha"))).toBe(false);
    expect(needsAttention(resourceByDisplayName("Fixture Database Beta"))).toBe(true);
    expect(needsAttention(resourceByDisplayName("Fixture Tombstoned Endpoint"))).toBe(true);
    expect(needsAttention(resourceByDisplayName("Fixture Orphaned Worker"))).toBe(true);
  });
});

describe("isExpired", () => {
  it("is true only for freshness === expired", () => {
    expect(isExpired(resourceByDisplayName("Fixture Compute Alpha"))).toBe(false);
    expect(isExpired(resourceByDisplayName("Fixture Database Beta"))).toBe(true);
    expect(isExpired(resourceByDisplayName("Fixture Tombstoned Endpoint"))).toBe(true);
    expect(isExpired(resourceByDisplayName("Fixture Orphaned Worker"))).toBe(false);
  });
});

describe("isRemoved", () => {
  it("is true only for tombstoned or orphaned lifecycle", () => {
    expect(isRemoved(resourceByDisplayName("Fixture Compute Alpha"))).toBe(false);
    expect(isRemoved(resourceByDisplayName("Fixture Database Beta"))).toBe(false);
    expect(isRemoved(resourceByDisplayName("Fixture Tombstoned Endpoint"))).toBe(true);
    expect(isRemoved(resourceByDisplayName("Fixture Orphaned Worker"))).toBe(true);
  });
});

describe("matchesStatusFilter", () => {
  const alpha = resourceByDisplayName("Fixture Compute Alpha");
  const beta = resourceByDisplayName("Fixture Database Beta");
  const tombstoned = resourceByDisplayName("Fixture Tombstoned Endpoint");
  const orphaned = resourceByDisplayName("Fixture Orphaned Worker");

  it('"all" keeps every resource', () => {
    for (const resource of [alpha, beta, tombstoned, orphaned]) {
      expect(matchesStatusFilter(resource, "all")).toBe(true);
    }
  });

  it('"attention" keeps anything not healthy/fresh/active', () => {
    expect(matchesStatusFilter(alpha, "attention")).toBe(false);
    expect(matchesStatusFilter(beta, "attention")).toBe(true);
    expect(matchesStatusFilter(tombstoned, "attention")).toBe(true);
    expect(matchesStatusFilter(orphaned, "attention")).toBe(true);
  });

  it('"expired" keeps only freshness-expired resources', () => {
    expect(matchesStatusFilter(alpha, "expired")).toBe(false);
    expect(matchesStatusFilter(beta, "expired")).toBe(true);
    expect(matchesStatusFilter(tombstoned, "expired")).toBe(true);
    expect(matchesStatusFilter(orphaned, "expired")).toBe(false);
  });

  it('"removed" keeps only tombstoned or orphaned resources', () => {
    expect(matchesStatusFilter(alpha, "removed")).toBe(false);
    expect(matchesStatusFilter(beta, "removed")).toBe(false);
    expect(matchesStatusFilter(tombstoned, "removed")).toBe(true);
    expect(matchesStatusFilter(orphaned, "removed")).toBe(true);
  });
});

describe("summarizeConnectionExpiry", () => {
  it("aggregates expired and removed counts per connection, union total", () => {
    const rows = summarizeConnectionExpiry(resources, snapshot.connections);
    expect(rows).toHaveLength(2);
    expect(rows[0].connection.connection_id).toBe("fixture-connection-alpha");
    expect(rows[0].expiredCount).toBe(1); // beta
    expect(rows[0].removedCount).toBe(0);
    expect(rows[0].total).toBe(1);
    expect(rows[1].connection.connection_id).toBe("fixture-connection-beta");
    expect(rows[1].expiredCount).toBe(1); // tombstoned
    expect(rows[1].removedCount).toBe(2); // tombstoned + orphaned
    expect(rows[1].total).toBe(3);
  });

  it("sorts rows by display_name with localeCompare en", () => {
    const rows = summarizeConnectionExpiry(resources, snapshot.connections);
    expect(rows.map((row) => row.connection.display_name)).toEqual([
      "Fixture Connection Alpha",
      "Fixture Connection Beta",
    ]);
  });

  it("skips connections without any expired/removed resources", () => {
    const rows = summarizeConnectionExpiry(resources, snapshot.connections);
    expect(rows.some((row) => row.connection.connection_id === "fixture-connection-disabled")).toBe(
      false,
    );
  });

  it("skips resources whose connection id is unknown", () => {
    const unknownConnection = {
      ...resources[0],
      connection_id: "fixture-connection-unknown",
    };
    const rows = summarizeConnectionExpiry(
      [unknownConnection, ...resources],
      snapshot.connections,
    );
    const alphaRow = rows.find(
      (row) => row.connection.connection_id === "fixture-connection-alpha",
    );
    expect(alphaRow?.expiredCount).toBe(1);
    expect(alphaRow?.removedCount).toBe(0);
  });
});
