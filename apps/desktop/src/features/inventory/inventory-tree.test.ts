import { describe, expect, it } from "vitest";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { FIXTURE_OBSERVED_AT } from "../../test/fixtures/query-fixtures";
import { buildResourceForest, flattenVisibleCount, type ResourceTreeNode } from "./inventory-tree";

function resource(
  resourceId: string,
  displayName: string,
  kind = "fixture.kind",
): ResourceDto {
  return {
    resource_id: resourceId,
    connection_id: "fixture-connection",
    kind,
    display_name: displayName,
    scope: "fixture-scope",
    lifecycle: "active",
    health: "healthy",
    freshness: "fresh",
    observed_at: FIXTURE_OBSERVED_AT,
  };
}

function relation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind = "fixture.contains",
  lifecycle: RelationDto["lifecycle"] = "active",
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
    lifecycle,
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: "github",
      connection_id: "fixture-github-connection",
      sync_run_id: "fixture-github-sync-run",
      field_path: "attributes.fixture_id",
    },
    last_seen_at: FIXTURE_OBSERVED_AT,
  };
}

describe("buildResourceForest", () => {
  it("uses containment for hierarchy and keeps runtime relations flat", () => {
    const resources = [
      resource("fixture-github-repository-10", "Fixture Repository", "github.repository"),
      resource("fixture-github-workflow-40", "Fixture Workflow", "github.workflow"),
      resource("fixture-github-run-50", "Fixture Run", "github.workflow_run"),
    ];
    const relations = [
      relation(
        "fixture-github-repository-workflow",
        "fixture-github-repository-10",
        "fixture-github-workflow-40",
        "github.contains",
      ),
      relation(
        "fixture-github-workflow-run",
        "fixture-github-workflow-40",
        "fixture-github-run-50",
        "github.executes",
      ),
    ];

    const forest = buildResourceForest(resources, relations);

    expect(forest).toHaveLength(2);
    const repository = forest.find(
      (node) => node.resource.resource_id === "fixture-github-repository-10",
    )!;
    const run = forest.find(
      (node) => node.resource.resource_id === "fixture-github-run-50",
    )!;
    expect(repository.children).toHaveLength(1);
    expect(repository.children[0].resource.resource_id).toBe("fixture-github-workflow-40");
    expect(repository.children[0].children).toHaveLength(0);
    expect(run.children).toHaveLength(0);
  });

  it("ignores non-hierarchy and inactive containment relations", () => {
    const resources = [
      resource("fixture-resource-alpha", "Fixture Alpha"),
      resource("fixture-resource-beta", "Fixture Beta"),
      resource("fixture-resource-gamma", "Fixture Gamma"),
    ];
    const relations = [
      relation(
        "fixture-relation-runtime",
        "fixture-resource-alpha",
        "fixture-resource-beta",
        "fixture.depends_on",
      ),
      relation(
        "fixture-relation-inactive",
        "fixture-resource-alpha",
        "fixture-resource-gamma",
        "fixture.contains",
        "tombstoned",
      ),
    ];

    const forest = buildResourceForest(resources, relations);

    expect(forest.map((node) => node.resource.resource_id)).toEqual([
      "fixture-resource-alpha",
      "fixture-resource-beta",
      "fixture-resource-gamma",
    ]);
  });

  it("keeps a target root when its relation points outside the visible page", () => {
    const resources = [
      resource("fixture-resource-alpha", "Fixture Alpha"),
      resource("fixture-resource-beta", "Fixture Beta"),
    ];
    const relations = [
      relation("fixture-relation-outside-parent", "fixture-resource-absent", "fixture-resource-beta"),
      relation("fixture-relation-outside-child", "fixture-resource-alpha", "fixture-resource-absent"),
    ];

    const forest = buildResourceForest(resources, relations);

    expect(forest).toHaveLength(2);
    const ids = forest.map((node) => node.resource.resource_id).sort();
    expect(ids).toEqual(["fixture-resource-alpha", "fixture-resource-beta"]);
    for (const node of forest) {
      expect(node.children).toHaveLength(0);
    }
  });

  it("sorts roots and siblings by display_name", () => {
    const resources = [
      resource("fixture-resource-zulu", "Zulu"),
      resource("fixture-resource-alpha", "Alpha"),
      resource("fixture-resource-bravo", "Bravo"),
      resource("fixture-resource-mike", "Mike"),
      resource("fixture-resource-delta", "Delta"),
    ];
    const relations = [
      relation("fixture-relation-alpha-mike", "fixture-resource-alpha", "fixture-resource-mike"),
      relation("fixture-relation-alpha-delta", "fixture-resource-alpha", "fixture-resource-delta"),
    ];

    const forest = buildResourceForest(resources, relations);

    expect(forest.map((node) => node.resource.display_name)).toEqual([
      "Alpha",
      "Bravo",
      "Zulu",
    ]);
    const alpha = forest.find((node) => node.resource.display_name === "Alpha")!;
    expect(alpha.children.map((child) => child.resource.display_name)).toEqual([
      "Delta",
      "Mike",
    ]);
  });

  it("breaks parent cycles so rendering never loops", () => {
    const resources = [
      resource("fixture-resource-alpha", "Fixture Alpha"),
      resource("fixture-resource-beta", "Fixture Beta"),
      resource("fixture-resource-gamma", "Fixture Gamma"),
    ];
    const relations = [
      relation("fixture-relation-a-b", "fixture-resource-alpha", "fixture-resource-beta"),
      relation("fixture-relation-b-a", "fixture-resource-beta", "fixture-resource-alpha"),
      relation("fixture-relation-c-a", "fixture-resource-gamma", "fixture-resource-alpha"),
    ];

    const forest = buildResourceForest(resources, relations);

    // Every resource must appear exactly once and no node may contain itself.
    const seen = new Set<string>();
    const walk = (nodes: readonly ResourceTreeNode[]) => {
      for (const node of nodes) {
        expect(seen.has(node.resource.resource_id)).toBe(false);
        seen.add(node.resource.resource_id);
        expect(node.resource.resource_id).not.toBe(node.children[0]?.resource.resource_id);
        walk(node.children);
      }
    };
    walk(forest);
    expect(seen.size).toBe(resources.length);
  });

  it("renders every resource at root level when there are no relations", () => {
    const resources = [
      resource("fixture-resource-beta", "Fixture Beta"),
      resource("fixture-resource-alpha", "Fixture Alpha"),
    ];

    const forest = buildResourceForest(resources, []);

    expect(forest.map((node) => node.resource.resource_id)).toEqual([
      "fixture-resource-alpha",
      "fixture-resource-beta",
    ]);
    expect(flattenVisibleCount(forest)).toBe(2);
  });
});
