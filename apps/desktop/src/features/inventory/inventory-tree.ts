import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";

export interface ResourceTreeNode {
  readonly resource: ResourceDto;
  readonly children: readonly ResourceTreeNode[];
}

/**
 * Builds a deterministic forest from the visible resources and their relations.
 *
 * Only active `*.contains` relations define hierarchy. For those edges,
 * `relation.source_resource_id` is the parent and `relation.target_resource_id` the child.
 * Runtime/dependency relations stay flat, and relations whose endpoints are not both visible
 * are ignored. Each sibling list — roots included — is sorted by display_name.
 *
 * Cycle defense: one parent per target, then an iterative detach pass breaks any parent
 * cycle by treating the repeated node as a root, so the walk can never loop.
 */
export function buildResourceForest(
  resources: readonly ResourceDto[],
  relations: readonly RelationDto[],
): readonly ResourceTreeNode[] {
  const byId = new Map<string, ResourceDto>();
  for (const resource of resources) {
    byId.set(resource.resource_id, resource);
  }

  const parentByTarget = new Map<string, string>();
  for (const relation of relations) {
    if (relation.lifecycle !== "active" || !relation.kind.endsWith(".contains")) continue;
    const source = relation.source_resource_id;
    const target = relation.target_resource_id;
    if (target === source) continue;
    if (!byId.has(source) || !byId.has(target)) continue;
    if (parentByTarget.has(target)) continue;
    parentByTarget.set(target, source);
  }

  detachCycles(parentByTarget);

  const childIdsByParent = new Map<string, string[]>();
  for (const [target, source] of parentByTarget) {
    const siblings = childIdsByParent.get(source);
    if (siblings === undefined) childIdsByParent.set(source, [target]);
    else siblings.push(target);
  }

  const buildNode = (
    resourceId: string,
    ancestors: ReadonlySet<string>,
  ): ResourceTreeNode => {
    const resource = byId.get(resourceId)!;
    const childIds = childIdsByParent.get(resourceId) ?? [];
    const children = childIds
      .filter((childId) => !ancestors.has(childId))
      .map((childId) => buildNode(childId, new Set(ancestors).add(resourceId)))
      .sort(compareByDisplayName);
    return { resource, children };
  };

  return resources
    .filter((resource) => !parentByTarget.has(resource.resource_id))
    .map((resource) => buildNode(resource.resource_id, new Set()))
    .sort(compareByDisplayName);
}

/** Detaches the closing edge of every parent cycle so the forest stays acyclic. */
function detachCycles(parentByTarget: Map<string, string>): void {
  for (const start of [...parentByTarget.keys()]) {
    const seen = new Set<string>();
    let current = start;
    while (parentByTarget.has(current) && !seen.has(current)) {
      seen.add(current);
      current = parentByTarget.get(current)!;
    }
    if (seen.has(current)) {
      parentByTarget.delete(current);
    }
  }
}

function compareByDisplayName(left: ResourceTreeNode, right: ResourceTreeNode): number {
  return left.resource.display_name.localeCompare(right.resource.display_name, "en");
}

/** Counts every node in the forest, collapsed or not. */
export function flattenVisibleCount(tree: readonly ResourceTreeNode[]): number {
  let count = 0;
  const stack = [...tree];
  while (stack.length > 0) {
    const node = stack.pop()!;
    count += 1;
    stack.push(...node.children);
  }
  return count;
}
