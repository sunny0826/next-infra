/**
 * Geometry for a grouped containment view. The helper deliberately has no
 * dependency on topology DTOs or on the DOM so that the page can choose how
 * to render the returned regions and positions.
 */

export const TOPOLOGY_HIERARCHY_CANVAS_WIDTH = 840;
export const TOPOLOGY_HIERARCHY_MIN_HEIGHT = 480;

const CANVAS_PADDING = 24;
const REGION_GAP = 24;
const REGION_WIDTH = TOPOLOGY_HIERARCHY_CANVAS_WIDTH - CANVAS_PADDING * 2;
const NODE_WIDTH = 136;
const NODE_HEIGHT = 64;
const NODE_GAP = 16;
const PARENT_COLUMNS = 5;
const GROUP_COLUMNS = 3;
const GROUP_GAP = 16;
const GROUP_HEADER_HEIGHT = 40;
const GROUP_VERTICAL_PADDING = 16;
const ITEM_HEIGHT = 48;
const ITEM_GAP = 8;

export interface TopologyHierarchyGroupInput {
  readonly id: string;
  readonly visibleItemCount: number;
  readonly totalLoadedCount: number;
  readonly expanded: boolean;
}

export interface TopologyHierarchyLayoutInput {
  readonly parentCount: number;
  readonly groups: readonly TopologyHierarchyGroupInput[];
}

export interface TopologyHierarchyRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

export interface TopologyHierarchyParentPosition {
  readonly index: number;
  readonly rect: TopologyHierarchyRect;
}

export interface TopologyHierarchyVisibleItemPosition {
  readonly groupId: string;
  readonly index: number;
  readonly x: number;
  readonly y: number;
}

export interface TopologyHierarchyGroupRectangle extends TopologyHierarchyRect {
  readonly id: string;
  readonly expanded: boolean;
  readonly visibleItemCount: number;
  readonly totalLoadedCount: number;
}

export interface TopologyHierarchyGroupLayout extends TopologyHierarchyGroupRectangle {
  readonly itemPositions: readonly TopologyHierarchyVisibleItemPosition[];
}

export interface TopologyHierarchyLayout {
  readonly canvasWidth: typeof TOPOLOGY_HIERARCHY_CANVAS_WIDTH;
  /** The required vertical canvas size, including the bottom canvas padding. */
  readonly height: number;
  readonly requiredCanvasHeight: number;
  readonly parentRegion: TopologyHierarchyRect;
  readonly focusRegion: TopologyHierarchyRect;
  readonly groupRegion: TopologyHierarchyRect;
  readonly regions: {
    readonly parent: TopologyHierarchyRect;
    readonly focus: TopologyHierarchyRect;
    readonly groups: TopologyHierarchyRect;
  };
  readonly parentPositions: readonly TopologyHierarchyParentPosition[];
  readonly focusRect: TopologyHierarchyRect;
  readonly groupRectangles: readonly TopologyHierarchyGroupRectangle[];
  readonly groups: readonly TopologyHierarchyGroupLayout[];
  readonly visibleItemPositions: readonly TopologyHierarchyVisibleItemPosition[];
}

interface NormalizedGroup extends TopologyHierarchyGroupInput {
  readonly visibleItemCount: number;
  readonly totalLoadedCount: number;
}

function nonNegativeCount(value: number): number {
  return Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
}

function normalizeGroup(group: TopologyHierarchyGroupInput): NormalizedGroup {
  return {
    id: group.id,
    visibleItemCount: nonNegativeCount(group.visibleItemCount),
    totalLoadedCount: nonNegativeCount(group.totalLoadedCount),
    expanded: group.expanded,
  };
}

function compareGroups(left: NormalizedGroup, right: NormalizedGroup): number {
  if (left.id < right.id) return -1;
  if (left.id > right.id) return 1;
  if (left.visibleItemCount !== right.visibleItemCount) {
    return left.visibleItemCount - right.visibleItemCount;
  }
  if (left.totalLoadedCount !== right.totalLoadedCount) {
    return left.totalLoadedCount - right.totalLoadedCount;
  }
  return Number(left.expanded) - Number(right.expanded);
}

function rowsFor(count: number, columns: number): number {
  return count === 0 ? 0 : Math.ceil(count / columns);
}

function parentRegionHeight(parentCount: number): number {
  const rows = rowsFor(parentCount, PARENT_COLUMNS);
  if (rows === 0) return NODE_HEIGHT;
  return rows * NODE_HEIGHT + Math.max(0, rows - 1) * REGION_GAP;
}

function groupHeight(group: NormalizedGroup): number {
  if (!group.expanded || group.visibleItemCount === 0) {
    return GROUP_HEADER_HEIGHT + GROUP_VERTICAL_PADDING * 2;
  }

  const itemHeight = group.visibleItemCount * ITEM_HEIGHT
    + (group.visibleItemCount - 1) * ITEM_GAP;
  return GROUP_HEADER_HEIGHT + GROUP_VERTICAL_PADDING * 2 + itemHeight;
}

function groupColumnCount(groupCount: number): number {
  return Math.min(GROUP_COLUMNS, groupCount);
}

function groupColumnWidth(columnCount: number): number {
  return (REGION_WIDTH - Math.max(0, columnCount - 1) * GROUP_GAP) / columnCount;
}

/**
 * Layout grouped containment content in parent, focus, and group regions.
 * Groups are sorted by id before placement; the input arrays are never
 * mutated. Collapsed groups render no item coordinates and use summary height
 * regardless of their loaded item count.
 */
export function layoutTopologyHierarchy(
  input: TopologyHierarchyLayoutInput,
): TopologyHierarchyLayout {
  const parentCount = nonNegativeCount(input.parentCount);
  const parentHeight = parentRegionHeight(parentCount);
  const parentRegion: TopologyHierarchyRect = {
    x: CANVAS_PADDING,
    y: CANVAS_PADDING,
    width: REGION_WIDTH,
    height: parentHeight,
  };

  const focusRegion: TopologyHierarchyRect = {
    x: CANVAS_PADDING,
    y: parentRegion.y + parentRegion.height + REGION_GAP,
    width: REGION_WIDTH,
    height: NODE_HEIGHT,
  };
  const focusRect: TopologyHierarchyRect = {
    x: (TOPOLOGY_HIERARCHY_CANVAS_WIDTH - NODE_WIDTH) / 2,
    y: focusRegion.y,
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
  };

  const normalizedGroups = input.groups.map(normalizeGroup).sort(compareGroups);
  const columns = groupColumnCount(normalizedGroups.length);
  const columnWidth = columns === 0 ? REGION_WIDTH : groupColumnWidth(columns);
  const rowHeights: number[] = [];

  normalizedGroups.forEach((group, index) => {
    const row = Math.floor(index / columns);
    rowHeights[row] = Math.max(rowHeights[row] ?? 0, groupHeight(group));
  });

  const groupRegionY = focusRegion.y + focusRegion.height + REGION_GAP;
  const groupRegionHeight = rowHeights.reduce(
    (height, rowHeight) => height + rowHeight,
    0,
  ) + Math.max(0, rowHeights.length - 1) * GROUP_GAP;
  const groupRegion: TopologyHierarchyRect = {
    x: CANVAS_PADDING,
    y: groupRegionY,
    width: REGION_WIDTH,
    height: groupRegionHeight,
  };

  const rowOffsets: number[] = [];
  rowHeights.reduce((offset, rowHeight, row) => {
    rowOffsets[row] = offset;
    return offset + rowHeight + GROUP_GAP;
  }, 0);

  const parentPositions: TopologyHierarchyParentPosition[] = Array.from(
    { length: parentCount },
    (_, index) => {
      const row = Math.floor(index / PARENT_COLUMNS);
      const column = index % PARENT_COLUMNS;
      const columnsInRow = Math.min(PARENT_COLUMNS, parentCount - row * PARENT_COLUMNS);
      const rowWidth = columnsInRow * NODE_WIDTH + (columnsInRow - 1) * NODE_GAP;
      return {
        index,
        rect: {
          x: CANVAS_PADDING + (REGION_WIDTH - rowWidth) / 2 + column * (NODE_WIDTH + NODE_GAP),
          y: parentRegion.y + row * (NODE_HEIGHT + REGION_GAP),
          width: NODE_WIDTH,
          height: NODE_HEIGHT,
        },
      };
    },
  );

  const groups: TopologyHierarchyGroupLayout[] = normalizedGroups.map((group, index) => {
    const row = Math.floor(index / columns);
    const column = index % columns;
    const rect: TopologyHierarchyGroupRectangle = {
      id: group.id,
      x: groupRegion.x + column * (columnWidth + GROUP_GAP),
      y: groupRegion.y + rowOffsets[row],
      width: columnWidth,
      height: groupHeight(group),
      expanded: group.expanded,
      visibleItemCount: group.visibleItemCount,
      totalLoadedCount: group.totalLoadedCount,
    };
    const itemPositions = group.expanded
      ? Array.from({ length: group.visibleItemCount }, (_, itemIndex) => ({
        groupId: group.id,
        index: itemIndex,
        x: rect.x + GROUP_VERTICAL_PADDING,
        y: rect.y + GROUP_HEADER_HEIGHT + GROUP_VERTICAL_PADDING
          + itemIndex * (ITEM_HEIGHT + ITEM_GAP),
      }))
      : [];
    return { ...rect, itemPositions };
  });

  const parent = parentRegion;
  const focus = focusRegion;
  const group = groupRegion;
  const visibleItemPositions = groups.flatMap((entry) => entry.itemPositions);
  const contentBottom = groupRegion.y + groupRegion.height + CANVAS_PADDING;
  const requiredCanvasHeight = Math.max(TOPOLOGY_HIERARCHY_MIN_HEIGHT, contentBottom);

  return {
    canvasWidth: TOPOLOGY_HIERARCHY_CANVAS_WIDTH,
    height: requiredCanvasHeight,
    requiredCanvasHeight,
    parentRegion: parent,
    focusRegion: focus,
    groupRegion: group,
    regions: { parent, focus, groups: group },
    parentPositions,
    focusRect,
    groupRectangles: groups.map(({ itemPositions: _itemPositions, ...rect }) => rect),
    groups,
    visibleItemPositions,
  };
}
