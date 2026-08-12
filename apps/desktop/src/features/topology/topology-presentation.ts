import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";

/**
 * A focus-relative containment endpoint. The relation is kept by reference so
 * callers can open the exact evidence record that produced the membership.
 */
export interface TopologyMembership {
  readonly resourceId: string;
  readonly resource: ResourceDto | null;
  readonly relation: RelationDto;
}

/** A stable group of children sharing the same target resource kind. */
export interface TopologyChildGroup {
  /** `null` is the explicit group for unresolved child endpoints. */
  readonly kind: string | null;
  readonly memberships: readonly TopologyMembership[];
}

/**
 * Focus-relative presentation data for a bounded topology result.
 *
 * `parentMemberships` and `childGroups` contain only `.contains` relations
 * incident to the focus. Every other visible relation is retained in
 * `operationalRelations`; this includes non-containment relations and
 * containment relations that are not incident to the focus.
 */
export interface TopologyPresentation {
  readonly focusResource: ResourceDto | null;
  readonly parentMemberships: readonly TopologyMembership[];
  readonly childGroups: readonly TopologyChildGroup[];
  readonly operationalRelations: readonly RelationDto[];
}

function compareText(left: string, right: string): number {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function compareMembership(left: TopologyMembership, right: TopologyMembership): number {
  const nameOrder = compareText(left.resource?.display_name ?? "", right.resource?.display_name ?? "");
  if (nameOrder !== 0) return nameOrder;

  const resourceOrder = compareText(left.resourceId, right.resourceId);
  if (resourceOrder !== 0) return resourceOrder;

  return compareText(left.relation.relation_id, right.relation.relation_id);
}

function compareGroup(left: TopologyChildGroup, right: TopologyChildGroup): number {
  if (left.kind === null && right.kind !== null) return 1;
  if (left.kind !== null && right.kind === null) return -1;
  return compareText(left.kind ?? "", right.kind ?? "");
}

function compareRelation(left: RelationDto, right: RelationDto): number {
  const sourceOrder = compareText(left.source_resource_id, right.source_resource_id);
  if (sourceOrder !== 0) return sourceOrder;

  const targetOrder = compareText(left.target_resource_id, right.target_resource_id);
  if (targetOrder !== 0) return targetOrder;

  const kindOrder = compareText(left.kind, right.kind);
  if (kindOrder !== 0) return kindOrder;

  const relationOrder = compareText(left.relation_id, right.relation_id);
  if (relationOrder !== 0) return relationOrder;

  const lifecycleOrder = compareText(left.lifecycle, right.lifecycle);
  if (lifecycleOrder !== 0) return lifecycleOrder;

  const evidenceTypeOrder = compareText(left.evidence_type, right.evidence_type);
  if (evidenceTypeOrder !== 0) return evidenceTypeOrder;

  return compareText(left.last_seen_at, right.last_seen_at);
}

function isContainment(relation: RelationDto): boolean {
  return relation.kind.endsWith(".contains");
}

function membership(
  endpointId: string,
  relation: RelationDto,
  resourcesById: ReadonlyMap<string, ResourceDto>,
): TopologyMembership {
  return {
    resourceId: endpointId,
    resource: resourcesById.get(endpointId) ?? null,
    relation,
  };
}

/**
 * Build deterministic, focus-relative topology presentation data.
 *
 * The caller supplies the already-visible relation set (including any
 * configured/tombstoned filtering it owns). This function only classifies
 * relation kinds and never mutates the topology or relation inputs.
 */
export function buildTopologyPresentation(
  topology: TopologyDto,
  visibleRelations: readonly RelationDto[],
): TopologyPresentation {
  const resourcesById = new Map<string, ResourceDto>();
  topology.nodes.forEach((resource) => resourcesById.set(resource.resource_id, resource));

  const parentMemberships: TopologyMembership[] = [];
  const childrenByKind = new Map<string | null, TopologyMembership[]>();
  const operationalRelations: RelationDto[] = [];
  const focusResource = resourcesById.get(topology.focus_resource_id) ?? null;

  visibleRelations.forEach((relation) => {
    if (!isContainment(relation)) {
      operationalRelations.push(relation);
      return;
    }

    const isIncoming = relation.target_resource_id === topology.focus_resource_id;
    const isOutgoing = relation.source_resource_id === topology.focus_resource_id;
    if (!isIncoming && !isOutgoing) {
      operationalRelations.push(relation);
      return;
    }

    if (isIncoming) {
      parentMemberships.push(
        membership(relation.source_resource_id, relation, resourcesById),
      );
    }

    if (isOutgoing) {
      const child = membership(relation.target_resource_id, relation, resourcesById);
      const kind = child.resource?.kind ?? null;
      childrenByKind.set(kind, [...(childrenByKind.get(kind) ?? []), child]);
    }
  });

  parentMemberships.sort(compareMembership);
  const childGroups = [...childrenByKind.entries()]
    .map(([kind, memberships]) => ({
      kind,
      memberships: [...memberships].sort(compareMembership),
    }))
    .sort(compareGroup);

  operationalRelations.sort(compareRelation);

  return {
    focusResource,
    parentMemberships,
    childGroups,
    operationalRelations,
  };
}
