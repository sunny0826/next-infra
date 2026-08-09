import { describe, expect, it } from "vitest";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import { buildTopologyPresentation } from "./topology-presentation";

const metadata = {
  schema_version: 1,
  snapshot_version: "fixture-snapshot",
  generated_at: "2026-08-09T00:00:00Z",
};

function resource(resourceId: string, kind: string, displayName: string): ResourceDto {
  return {
    resource_id: resourceId,
    connection_id: "fixture-connection",
    kind,
    display_name: displayName,
    scope: "fixture-scope",
    lifecycle: "active",
    health: "healthy",
    freshness: "fresh",
    observed_at: "2026-08-09T00:00:00Z",
  };
}

function relation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind: string,
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
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

function topology(nodes: ResourceDto[], edges: RelationDto[]): TopologyDto {
  return {
    metadata,
    focus_resource_id: "fixture-focus",
    depth: 1,
    nodes,
    edges,
    frontier: [],
    truncated: false,
  };
}

describe("topology presentation", () => {
  it("groups child memberships by target kind", () => {
    const focus = resource("fixture-focus", "fixture.host", "Focus");
    const service = resource("fixture-service", "fixture.service", "Service");
    const database = resource("fixture-database", "fixture.database", "Database");
    const serviceRelation = relation("fixture-contains-service", focus.resource_id, service.resource_id, "fixture.contains");
    const databaseRelation = relation("fixture-contains-database", focus.resource_id, database.resource_id, "fixture.contains");

    const result = buildTopologyPresentation(
      topology([database, focus, service], [databaseRelation, serviceRelation]),
      [databaseRelation, serviceRelation],
    );

    expect(result.childGroups.map((group) => group.kind)).toEqual([
      "fixture.database",
      "fixture.service",
    ]);
    expect(result.childGroups[0]?.memberships[0]).toMatchObject({
      resource: database,
      relation: databaseRelation,
    });
    expect(result.childGroups[1]?.memberships[0]).toMatchObject({
      resource: service,
      relation: serviceRelation,
    });
  });

  it("exposes parent membership when the focus is a child", () => {
    const parent = resource("fixture-parent", "fixture.project", "Parent");
    const focus = resource("fixture-focus", "fixture.service", "Focus");
    const contains = relation("fixture-parent-contains-focus", parent.resource_id, focus.resource_id, "fixture.contains");

    const result = buildTopologyPresentation(
      topology([focus, parent], [contains]),
      [contains],
    );

    expect(result.parentMemberships).toEqual([{
      resourceId: parent.resource_id,
      resource: parent,
      relation: contains,
    }]);
    expect(result.childGroups).toEqual([]);
  });

  it("keeps executes operational instead of treating it as containment", () => {
    const focus = resource("fixture-focus", "fixture.workflow", "Focus");
    const run = resource("fixture-run", "fixture.run", "Run");
    const executes = relation("fixture-executes", focus.resource_id, run.resource_id, "github.executes");

    const result = buildTopologyPresentation(topology([focus, run], [executes]), [executes]);

    expect(result.childGroups).toEqual([]);
    expect(result.parentMemberships).toEqual([]);
    expect(result.operationalRelations).toEqual([executes]);
  });

  it("keeps unknown relation kinds in operational fallback", () => {
    const focus = resource("fixture-focus", "fixture.resource", "Focus");
    const target = resource("fixture-target", "fixture.resource", "Target");
    const unknown = relation("fixture-unknown", focus.resource_id, target.resource_id, "fixture.mystery");

    const result = buildTopologyPresentation(topology([focus, target], [unknown]), [unknown]);

    expect(result.operationalRelations).toEqual([unknown]);
  });

  it("retains a missing endpoint as an unresolved membership", () => {
    const focus = resource("fixture-focus", "fixture.project", "Focus");
    const missingChildId = "fixture-missing-child";
    const contains = relation("fixture-missing-child-edge", focus.resource_id, missingChildId, "fixture.contains");

    const result = buildTopologyPresentation(topology([focus], [contains]), [contains]);
    const membership = result.childGroups[0]?.memberships[0];

    expect(result.childGroups).toHaveLength(1);
    expect(result.childGroups[0]?.kind).toBeNull();
    expect(membership).toMatchObject({
      resourceId: missingChildId,
      resource: null,
      relation: contains,
    });
  });

  it("is deterministic without mutating topology nodes or visible relations", () => {
    const focus = resource("fixture-focus", "fixture.project", "Focus");
    const childA = resource("fixture-a", "fixture.service", "Alpha");
    const childB = resource("fixture-b", "fixture.service", "Beta");
    const parent = resource("fixture-parent", "fixture.project", "Parent");
    const childRelationA = relation("fixture-child-a", focus.resource_id, childA.resource_id, "fixture.contains");
    const childRelationB = relation("fixture-child-b", focus.resource_id, childB.resource_id, "fixture.contains");
    const parentRelation = relation("fixture-parent", parent.resource_id, focus.resource_id, "fixture.contains");
    const operational = relation("fixture-operational", childA.resource_id, childB.resource_id, "fixture.depends_on");
    const nodes = [childB, focus, parent, childA];
    const visibleRelations = [operational, childRelationB, parentRelation, childRelationA];
    const originalNodeOrder = [...nodes];
    const originalRelationOrder = [...visibleRelations];
    const first = buildTopologyPresentation(topology(nodes, visibleRelations), visibleRelations);
    const second = buildTopologyPresentation(
      topology([...nodes].reverse(), [...visibleRelations].reverse()),
      [...visibleRelations].reverse(),
    );

    expect(second).toEqual(first);
    expect(nodes).toEqual(originalNodeOrder);
    expect(visibleRelations).toEqual(originalRelationOrder);
    expect(first.childGroups[0]?.memberships.map(({ resource }) => resource?.display_name)).toEqual([
      "Alpha",
      "Beta",
    ]);
    expect(first.parentMemberships[0]?.resource?.display_name).toBe("Parent");
  });
});
