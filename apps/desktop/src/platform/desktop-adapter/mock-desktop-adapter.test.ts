import { createDesktopAdapterSnapshotFixture } from "../../test/fixtures/desktop-adapter-snapshot";
import { describe, expect, it } from "vitest";

import { MockDesktopAdapter } from "./mock-desktop-adapter";

describe("MockDesktopAdapter", () => {
  it("returns the injected deterministic snapshot", async () => {
    const fixture = createDesktopAdapterSnapshotFixture();
    const adapter = new MockDesktopAdapter(fixture);

    await expect(adapter.searchResources()).resolves.toMatchObject({
      metadata: fixture.metadata,
      items: fixture.resources,
    });
    await expect(adapter.listConnections()).resolves.toEqual({
      metadata: fixture.metadata,
      items: fixture.connections,
    });
    await expect(
      adapter.getTopology({ focus_resource_id: fixture.resources[0].resource_id }),
    ).resolves.toMatchObject({
      metadata: fixture.metadata,
      nodes: fixture.resources,
      edges: fixture.relations,
    });
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

    const firstResources = await adapter.searchResources();
    const firstConnections = await adapter.listConnections();
    firstResources.items[0].display_name = "Mutated consumer result";
    firstResources.metadata.snapshot_version = "mutated-consumer-version";
    firstConnections.items[0].display_name = "Mutated consumer connection";
    firstConnections.metadata.snapshot_version = "mutated-consumer-version";

    const secondResources = await adapter.searchResources();
    const secondConnections = await adapter.listConnections();

    expect(secondResources.items).toEqual(expected.resources);
    expect(secondResources.metadata).toEqual(expected.metadata);
    expect(secondConnections.items).toEqual(expected.connections);
    expect(secondConnections.metadata).toEqual(expected.metadata);
    expect(secondResources.items).not.toBe(firstResources.items);
    expect(secondConnections.items).not.toBe(firstConnections.items);
    expect(secondResources.items[0]).not.toBe(firstResources.items[0]);
    expect(secondConnections.items[0]).not.toBe(firstConnections.items[0]);
    expect(secondResources.metadata).not.toBe(firstResources.metadata);
    expect(secondConnections.metadata).not.toBe(firstConnections.metadata);
  });

  it("filters snapshot relations by the requested resource ids", async () => {
    const fixture = createDesktopAdapterSnapshotFixture();
    const adapter = new MockDesktopAdapter(fixture);

    await expect(
      adapter.getRelationsForResources({
        resource_ids: ["fixture-resource-alpha", "fixture-resource-beta"],
      }),
    ).resolves.toEqual({
      metadata: fixture.metadata,
      items: fixture.relations,
      page_info: { next_cursor: null },
    });

    await expect(
      adapter.getRelationsForResources({ resource_ids: ["fixture-resource-alpha"] }),
    ).resolves.toEqual({
      metadata: fixture.metadata,
      items: [],
      page_info: { next_cursor: null },
    });

    await expect(
      adapter.getRelationsForResources({
        resource_ids: ["fixture-resource-absent"],
      }),
    ).resolves.toEqual({
      metadata: fixture.metadata,
      items: [],
      page_info: { next_cursor: null },
    });
  });
});
