import type { RelationDto } from "../../generated/query/RelationDto";

export const TOPOLOGY_CANVAS_WIDTH = 840;
export const TOPOLOGY_NODE_WIDTH = 136;
export const TOPOLOGY_NODE_HEIGHT = 64;

const LANE_X = {
  incoming: 48,
  focus: 352,
  outgoing: 656,
  context: 352,
} as const;
const CONTEXT_X = [48, 200, 352, 504, 656] as const;
const FIRST_ROW_Y = 92;
const ROW_GAP = 92;

export type TopologyLane = keyof typeof LANE_X;

export interface TopologyLayoutNode {
  readonly resourceId: string;
  readonly lane: TopologyLane;
  readonly x: number;
  readonly y: number;
}

export interface TopologyLayout {
  readonly height: number;
  readonly nodes: ReadonlyMap<string, TopologyLayoutNode>;
}

function laneForResource(
  resourceId: string,
  focusResourceId: string,
  relations: readonly RelationDto[],
): TopologyLane {
  if (resourceId === focusResourceId) return "focus";
  if (relations.some((relation) => relation.target_resource_id === focusResourceId
    && relation.source_resource_id === resourceId)) return "incoming";
  if (relations.some((relation) => relation.source_resource_id === focusResourceId
    && relation.target_resource_id === resourceId)) return "outgoing";
  return "context";
}

export function layoutTopology(
  resourceIds: readonly string[],
  focusResourceId: string,
  relations: readonly RelationDto[],
): TopologyLayout {
  const lanes: Record<TopologyLane, string[]> = {
    incoming: [],
    focus: [],
    outgoing: [],
    context: [],
  };

  [...new Set(resourceIds)].sort().forEach((resourceId) => {
    lanes[laneForResource(resourceId, focusResourceId, relations)].push(resourceId);
  });

  const sideRows = Math.max(lanes.incoming.length, lanes.outgoing.length, 1);
  const focusY = FIRST_ROW_Y + ((sideRows - 1) * ROW_GAP) / 2;
  const contextStartY = FIRST_ROW_Y + sideRows * ROW_GAP + 28;
  const nodes = new Map<string, TopologyLayoutNode>();

  (["incoming", "outgoing"] as const).forEach((lane) => {
    lanes[lane].forEach((resourceId, index) => {
      nodes.set(resourceId, {
        resourceId,
        lane,
        x: LANE_X[lane],
        y: FIRST_ROW_Y + index * ROW_GAP,
      });
    });
  });

  lanes.focus.forEach((resourceId, index) => {
    nodes.set(resourceId, {
      resourceId,
      lane: "focus",
      x: LANE_X.focus,
      y: focusY + index * ROW_GAP,
    });
  });

  lanes.context.forEach((resourceId, index) => {
    nodes.set(resourceId, {
      resourceId,
      lane: "context",
      x: CONTEXT_X[index % CONTEXT_X.length],
      y: contextStartY + Math.floor(index / CONTEXT_X.length) * ROW_GAP,
    });
  });

  const contentBottom = Math.max(
    ...[...nodes.values()].map((node) => node.y + TOPOLOGY_NODE_HEIGHT),
    FIRST_ROW_Y + TOPOLOGY_NODE_HEIGHT,
  );

  return {
    height: Math.max(480, contentBottom + 104),
    nodes,
  };
}

export function parallelOffset(index: number, count: number): number {
  return (index - (count - 1) / 2) * 12;
}

export function relationCurve(
  source: TopologyLayoutNode,
  target: TopologyLayoutNode,
  offset = 0,
): string {
  const travelsRight = source.x <= target.x;
  const x1 = travelsRight ? source.x + TOPOLOGY_NODE_WIDTH : source.x;
  const x2 = travelsRight ? target.x : target.x + TOPOLOGY_NODE_WIDTH;
  const y1 = source.y + TOPOLOGY_NODE_HEIGHT / 2 + offset;
  const y2 = target.y + TOPOLOGY_NODE_HEIGHT / 2 + offset;
  const controlX = (x1 + x2) / 2;
  return `M${x1},${y1} C${controlX},${y1} ${controlX},${y2} ${x2},${y2}`;
}

export function relationLabelPoint(
  source: TopologyLayoutNode,
  target: TopologyLayoutNode,
  offset = 0,
): { readonly x: number; readonly y: number } {
  const sourceAnchor = source.x <= target.x
    ? source.x + TOPOLOGY_NODE_WIDTH
    : source.x;
  const targetAnchor = source.x <= target.x
    ? target.x
    : target.x + TOPOLOGY_NODE_WIDTH;
  return {
    x: (sourceAnchor + targetAnchor) / 2,
    y: (source.y + target.y) / 2 + TOPOLOGY_NODE_HEIGHT / 2 + offset - 7,
  };
}
