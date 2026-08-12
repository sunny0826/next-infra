import { describe, expect, it } from "vitest";

import type { RelationDto } from "../../generated/query/RelationDto";
import {
  layoutTopology,
  parallelOffset,
  relationCurve,
} from "./topology-layout";

function relation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: "fixture",
      connection_id: "fixture-connection",
      sync_run_id: "fixture-sync-run",
      field_path: "fixture.path",
    },
    last_seen_at: "2026-08-09T00:00:00Z",
  };
}

describe("topology focus-corridor layout", () => {
  it("places incoming left, focus center, and outgoing right deterministically", () => {
    const relations = [
      relation("fixture-relation-2", "fixture-upstream-b", "fixture-focus"),
      relation("fixture-relation-1", "fixture-upstream-a", "fixture-focus"),
      relation("fixture-relation-3", "fixture-focus", "fixture-downstream"),
    ];
    const layout = layoutTopology([
      "fixture-downstream",
      "fixture-focus",
      "fixture-upstream-b",
      "fixture-upstream-a",
    ], "fixture-focus", relations);

    expect(layout.nodes.get("fixture-upstream-a")).toMatchObject({ lane: "incoming", x: 48, y: 92 });
    expect(layout.nodes.get("fixture-upstream-b")).toMatchObject({ lane: "incoming", x: 48, y: 184 });
    expect(layout.nodes.get("fixture-focus")).toMatchObject({ lane: "focus", x: 352, y: 138 });
    expect(layout.nodes.get("fixture-downstream")).toMatchObject({ lane: "outgoing", x: 656, y: 92 });
  });

  it("keeps parallel relationships visually separated", () => {
    expect([0, 1, 2].map((index) => parallelOffset(index, 3))).toEqual([-12, 0, 12]);
  });

  it("uses a curved path between the correct node-facing anchors", () => {
    const layout = layoutTopology(
      ["fixture-source", "fixture-focus"],
      "fixture-focus",
      [relation("fixture-relation", "fixture-source", "fixture-focus")],
    );
    const source = layout.nodes.get("fixture-source");
    const target = layout.nodes.get("fixture-focus");
    expect(source).toBeDefined();
    expect(target).toBeDefined();
    expect(relationCurve(source!, target!)).toBe("M184,124 C268,124 268,124 352,124");
  });
});
