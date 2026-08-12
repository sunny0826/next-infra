import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import type {
  GetTopologyInput,
} from "../../platform/desktop-adapter/desktop-adapter";
import {
  MockDesktopAdapter,
  type DesktopAdapterSnapshot,
} from "../../platform/desktop-adapter/mock-desktop-adapter";
import {
  createTopologyHierarchySnapshotFixture,
  TOPOLOGY_HIERARCHY_FIXTURE_IDS,
} from "./topology-hierarchy-fixture";

export const TOPOLOGY_HIERARCHY_DEFAULT_DEPTH = 1;
export const TOPOLOGY_HIERARCHY_DEFAULT_MAX_NODES = 100;
export const TOPOLOGY_HIERARCHY_DEFAULT_MAX_EDGES = 200;
export const TOPOLOGY_HIERARCHY_HARD_MAX_NODES = 200;
export const TOPOLOGY_HIERARCHY_HARD_MAX_EDGES = 400;

function copyResource(resource: ResourceDto): ResourceDto {
  return { ...resource };
}

function copyRelation(relation: RelationDto): RelationDto {
  return {
    ...relation,
    evidence: relation.evidence.type === "provider"
      ? { ...relation.evidence }
      : relation.evidence.type === "configured"
        ? { ...relation.evidence }
        : {
            ...relation.evidence,
            input_resource_version_ids: [...relation.evidence.input_resource_version_ids],
            input_relation_version_ids: [...relation.evidence.input_relation_version_ids],
          },
  };
}

function copySnapshot(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    metadata: snapshot.metadata === null ? null : { ...snapshot.metadata },
    resources: snapshot.resources.map(copyResource),
    relations: snapshot.relations.map(copyRelation),
    connections: snapshot.connections.map((connection) => ({ ...connection })),
  };
}

function boundedValue(
  value: number | undefined,
  fallback: number,
  maximum: number,
  field: string,
): number {
  const resolved = value ?? fallback;
  if (!Number.isInteger(resolved) || resolved < 1 || resolved > maximum) {
    throw new Error(`${field} is outside the supported fixture range.`);
  }
  return resolved;
}

function relationDirection(
  relation: RelationDto,
  focusResourceId: string,
): "incoming" | "outgoing" | null {
  if (relation.source_resource_id === focusResourceId && relation.target_resource_id !== focusResourceId) {
    return "outgoing";
  }
  if (relation.target_resource_id === focusResourceId && relation.source_resource_id !== focusResourceId) {
    return "incoming";
  }
  return null;
}

function relationNeighborId(relation: RelationDto, direction: "incoming" | "outgoing"): string {
  return direction === "outgoing"
    ? relation.target_resource_id
    : relation.source_resource_id;
}

/**
 * A local test adapter with the same bounded shape as the query topology
 * endpoint. Only direct focus adjacency is considered; unrelated snapshot
 * resources and edges never leak into the response.
 */
export class TopologyHierarchyAdapter extends MockDesktopAdapter {
  readonly #snapshot: DesktopAdapterSnapshot;

  constructor(snapshot: DesktopAdapterSnapshot = createTopologyHierarchySnapshotFixture()) {
    super(snapshot);
    this.#snapshot = copySnapshot(snapshot);
    if (this.#snapshot.metadata === null) {
      throw new Error("Topology hierarchy fixture metadata is unavailable.");
    }
  }

  override async getTopology(input: GetTopologyInput): Promise<TopologyDto> {
    const depth = input.depth ?? TOPOLOGY_HIERARCHY_DEFAULT_DEPTH;
    if (depth !== TOPOLOGY_HIERARCHY_DEFAULT_DEPTH) {
      throw new Error("Topology hierarchy fixture supports depth 1 only.");
    }

    const maxNodes = boundedValue(
      input.max_nodes,
      TOPOLOGY_HIERARCHY_DEFAULT_MAX_NODES,
      TOPOLOGY_HIERARCHY_HARD_MAX_NODES,
      "max_nodes",
    );
    const maxEdges = boundedValue(
      input.max_edges,
      TOPOLOGY_HIERARCHY_DEFAULT_MAX_EDGES,
      TOPOLOGY_HIERARCHY_HARD_MAX_EDGES,
      "max_edges",
    );

    const focus = this.#snapshot.resources.find(
      (resource) => resource.resource_id === input.focus_resource_id,
    );
    if (focus === undefined) {
      throw new Error("Fixture topology focus was not found.");
    }

    const resourcesById = new Map<string, ResourceDto>(
      this.#snapshot.resources.map((resource) => [resource.resource_id, resource] as const),
    );
    const nodeIds = new Set<string>([focus.resource_id]);
    const edges: RelationDto[] = [];
    const frontier: TopologyDto["frontier"] = [];
    const frontierKeys = new Set<string>();
    let truncated = false;

    const incidentRelations = this.#snapshot.relations
      .filter((relation) => relation.lifecycle !== "tombstoned")
      .map((relation) => ({
        relation,
        direction: relationDirection(relation, focus.resource_id),
      }))
      .filter((entry): entry is { relation: RelationDto; direction: "incoming" | "outgoing" } =>
        entry.direction !== null,
      )
      .sort((left, right) => left.relation.relation_id.localeCompare(right.relation.relation_id));

    const addFrontier = (resourceId: string, direction: "incoming" | "outgoing") => {
      const key = `${resourceId}:${direction}`;
      if (!frontierKeys.has(key)) {
        frontierKeys.add(key);
        frontier.push({ resource_id: resourceId, direction });
      }
    };

    for (const { relation, direction } of incidentRelations) {
      const neighborId = relationNeighborId(relation, direction);
      const neighbor = resourcesById.get(neighborId);
      const hasPresentNeighbor = neighbor !== undefined && neighbor.lifecycle !== "tombstoned";

      if (hasPresentNeighbor && !nodeIds.has(neighborId) && nodeIds.size >= maxNodes) {
        truncated = true;
        addFrontier(neighborId, direction);
        continue;
      }

      if (edges.length >= maxEdges) {
        truncated = true;
        if (hasPresentNeighbor) {
          addFrontier(neighborId, direction);
        }
        continue;
      }

      if (hasPresentNeighbor) {
        nodeIds.add(neighborId);
      }
      edges.push(copyRelation(relation));
    }

    const nodes = [
      copyResource(focus),
      ...[...nodeIds]
        .filter((resourceId) => resourceId !== focus.resource_id)
        .map((resourceId) => resourcesById.get(resourceId))
        .filter((resource): resource is ResourceDto => resource !== undefined)
        .map(copyResource),
    ];

    return {
      metadata: this.#metadata(),
      focus_resource_id: focus.resource_id,
      depth,
      nodes,
      edges,
      frontier,
      truncated,
    };
  }

  #metadata() {
    if (this.#snapshot.metadata === null) {
      throw new Error("Topology hierarchy fixture metadata is unavailable.");
    }
    return { ...this.#snapshot.metadata };
  }
}

export function createTopologyHierarchyAdapter(
  snapshot: DesktopAdapterSnapshot = createTopologyHierarchySnapshotFixture(),
): TopologyHierarchyAdapter {
  return new TopologyHierarchyAdapter(snapshot);
}

export {
  createTopologyHierarchySnapshotFixture,
  TOPOLOGY_HIERARCHY_FIXTURE_IDS,
  TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
} from "./topology-hierarchy-fixture";
