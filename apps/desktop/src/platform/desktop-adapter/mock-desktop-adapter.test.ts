import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { createDesktopAdapterSnapshotFixture } from "../../test/fixtures/desktop-adapter-snapshot";
import { describe, expect, it } from "vitest";

import { MockDesktopAdapter } from "./mock-desktop-adapter";

describe("MockDesktopAdapter", () => {
  it("returns the injected deterministic snapshot", async () => {
    const fixture = createDesktopAdapterSnapshotFixture();
    const adapter = new MockDesktopAdapter(fixture);

    await expect(adapter.getSnapshotMetadata()).resolves.toEqual(fixture.metadata);
    await expect(adapter.listResources()).resolves.toEqual(fixture.resources);
    await expect(adapter.listRelations()).resolves.toEqual(fixture.relations);
    await expect(adapter.listConnections()).resolves.toEqual(fixture.connections);
  });

  it("isolates its snapshot from constructor and consumer mutations", async () => {
    const fixture = createDesktopAdapterSnapshotFixture();
    const expected = createDesktopAdapterSnapshotFixture();
    const adapter = new MockDesktopAdapter(fixture);

    fixture.resources[0].display_name = "Mutated constructor input";
    fixture.relations[0].kind = "mutated_constructor_relation";
    fixture.connections[0].display_name = "Mutated constructor connection";
    if (fixture.metadata !== null) {
      fixture.metadata.snapshot_version = "mutated-constructor-version";
    }

    const firstResources = await adapter.listResources();
    const firstRelations = await adapter.listRelations();
    const firstConnections = await adapter.listConnections();
    const firstMetadata = await adapter.getSnapshotMetadata();
    (firstResources as ResourceDto[]).push({ ...firstResources[0] });
    (firstRelations as RelationDto[]).push({ ...firstRelations[0] });
    (firstConnections as ConnectionDto[]).push({ ...firstConnections[0] });
    firstResources[0].display_name = "Mutated consumer result";
    firstRelations[0].kind = "mutated_consumer_relation";
    firstConnections[0].display_name = "Mutated consumer connection";
    if (firstMetadata !== null) {
      firstMetadata.snapshot_version = "mutated-consumer-version";
    }

    const secondResources = await adapter.listResources();
    const secondRelations = await adapter.listRelations();
    const secondConnections = await adapter.listConnections();
    const secondMetadata = await adapter.getSnapshotMetadata();

    expect(secondResources).toEqual(expected.resources);
    expect(secondRelations).toEqual(expected.relations);
    expect(secondConnections).toEqual(expected.connections);
    expect(secondMetadata).toEqual(expected.metadata);
    expect(secondResources).not.toBe(firstResources);
    expect(secondRelations).not.toBe(firstRelations);
    expect(secondConnections).not.toBe(firstConnections);
    expect(secondResources[0]).not.toBe(firstResources[0]);
    expect(secondRelations[0]).not.toBe(firstRelations[0]);
    expect(secondConnections[0]).not.toBe(firstConnections[0]);
    expect(secondMetadata).not.toBe(firstMetadata);
  });
});
