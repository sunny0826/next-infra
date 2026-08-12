import { describe, expect, it } from "vitest";

import {
  layoutTopologyHierarchy,
  TOPOLOGY_HIERARCHY_CANVAS_WIDTH,
} from "./topology-hierarchy-layout";

function group(
  id: string,
  visibleItemCount = 2,
  totalLoadedCount = visibleItemCount,
  expanded = false,
) {
  return { id, visibleItemCount, totalLoadedCount, expanded } as const;
}

describe("grouped containment hierarchy layout", () => {
  it.each([1, 3, 6])("returns one layout per group for %i groups", (count) => {
    const layout = layoutTopologyHierarchy({
      parentCount: count,
      groups: Array.from({ length: count }, (_, index) => group(`group-${index}`)),
    });

    expect(layout.canvasWidth).toBe(TOPOLOGY_HIERARCHY_CANVAS_WIDTH);
    expect(layout.parentPositions).toHaveLength(count);
    expect(layout.groups).toHaveLength(count);
    expect(layout.groupRectangles).toHaveLength(count);
    expect(layout.visibleItemPositions).toHaveLength(0);
    expect(layout.height).toBe(layout.requiredCanvasHeight);
  });

  it("keeps a collapsed group summary-sized even with 60 loaded items", () => {
    const layout = layoutTopologyHierarchy({
      parentCount: 0,
      groups: [group("large", 60, 60, false)],
    });

    expect(layout.groups[0]?.itemPositions).toEqual([]);
    expect(layout.visibleItemPositions).toEqual([]);
    expect(layout.groups[0]?.height).toBe(72);
    expect(layout.requiredCanvasHeight).toBeLessThan(840);
  });

  it("grows only the expanded group's rectangle and item positions", () => {
    const collapsed = layoutTopologyHierarchy({
      parentCount: 1,
      groups: [group("a"), group("b"), group("c")],
    });
    const expanded = layoutTopologyHierarchy({
      parentCount: 1,
      groups: [group("a"), group("b", 4, 8, true), group("c")],
    });

    const collapsedById = new Map(collapsed.groups.map((entry) => [entry.id, entry]));
    const expandedById = new Map(expanded.groups.map((entry) => [entry.id, entry]));
    expect(expandedById.get("b")?.height).toBeGreaterThan(collapsedById.get("b")?.height ?? 0);
    expect(expandedById.get("b")?.itemPositions).toHaveLength(4);
    expect(
      (expandedById.get("b")?.itemPositions[1]?.y ?? 0)
        - (expandedById.get("b")?.itemPositions[0]?.y ?? 0),
    ).toBeGreaterThanOrEqual(48);
    expect(expandedById.get("a")?.height).toBe(collapsedById.get("a")?.height);
    expect(expandedById.get("c")?.height).toBe(collapsedById.get("c")?.height);
    expect(expanded.visibleItemPositions).toHaveLength(4);
  });

  it("handles empty, single, and long groups", () => {
    const empty = layoutTopologyHierarchy({ parentCount: 0, groups: [] });
    const single = layoutTopologyHierarchy({
      parentCount: 1,
      groups: [group("single", 0, 0, true)],
    });
    const long = layoutTopologyHierarchy({
      parentCount: 1,
      groups: [group("long", 25, 100, true)],
    });

    expect(empty.groups).toEqual([]);
    expect(empty.parentRegion.height).toBe(64);
    expect(empty.focusRegion.y).toBeGreaterThan(
      empty.parentRegion.y + empty.parentRegion.height,
    );
    expect(empty.requiredCanvasHeight).toBe(480);
    expect(single.groups[0]?.itemPositions).toEqual([]);
    expect(long.groups[0]?.itemPositions).toHaveLength(25);
    expect(long.requiredCanvasHeight).toBeGreaterThan(single.requiredCanvasHeight);
  });

  it("is deterministic for reordered input and does not mutate it", () => {
    const groups = [group("z", 3, 5), group("a", 1, 2), group("m", 2, 3, true)];
    const original = groups.map((entry) => ({ ...entry }));
    const first = layoutTopologyHierarchy({ parentCount: 7, groups });
    const second = layoutTopologyHierarchy({
      parentCount: 7,
      groups: [...groups].reverse(),
    });

    expect(groups).toEqual(original);
    expect(first).toEqual(second);
    expect(first.groups.map((entry) => entry.id)).toEqual(["a", "m", "z"]);
  });
});
