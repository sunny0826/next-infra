import { describe, expect, it } from "vitest";

import { EmptyDesktopAdapter } from "./empty-desktop-adapter";

describe("EmptyDesktopAdapter", () => {
  it("returns an empty read-only snapshot", async () => {
    const adapter = new EmptyDesktopAdapter();
    const [metadata, resources, relations, connections] = await Promise.all([
      adapter.getSnapshotMetadata(),
      adapter.listResources(),
      adapter.listRelations(),
      adapter.listConnections(),
    ]);

    expect(metadata).toBeNull();
    expect(resources).toEqual([]);
    expect(relations).toEqual([]);
    expect(connections).toEqual([]);
    expect(Object.isFrozen(resources)).toBe(true);
    expect(Object.isFrozen(relations)).toBe(true);
    expect(Object.isFrozen(connections)).toBe(true);
  });
});
