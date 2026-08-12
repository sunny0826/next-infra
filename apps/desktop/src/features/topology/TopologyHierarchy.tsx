import { useLayoutEffect, useRef } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import {
  layoutTopologyHierarchy,
  type TopologyHierarchyGroupLayout,
} from "./topology-hierarchy-layout";
import type {
  TopologyChildGroup,
  TopologyMembership,
  TopologyPresentation,
} from "./topology-presentation";

interface TopologyHierarchyProps {
  readonly expandedGroups: ReadonlySet<string>;
  readonly onFocusResource?: (resourceId: string) => void;
  readonly onInspectRelation: (relation: RelationDto) => void;
  readonly onInspectResource: (resource: ResourceDto) => void;
  readonly onToggleGroup: (groupId: string) => void;
  readonly presentation: TopologyPresentation;
  readonly selectedRelationId?: string;
  readonly selectedResourceId?: string;
  readonly truncated: boolean;
}

function groupId(group: TopologyChildGroup): string {
  return group.kind === null ? "unresolved" : `kind:${group.kind}`;
}

function groupLabel(group: TopologyChildGroup): string {
  return group.kind ?? "未解析资源";
}

function resourceClass(resource: ResourceDto, selected: boolean): string {
  return `topology-hierarchy-resource topology-node--health-${resource.health}${selected ? " is-selected" : ""}`;
}

function MembershipActions({
  membership,
  onFocusResource,
  onInspectRelation,
  onInspectResource,
  selectedRelationId,
  selectedResourceId,
}: {
  readonly membership: TopologyMembership;
  readonly onFocusResource?: (resourceId: string) => void;
  readonly onInspectRelation: (relation: RelationDto) => void;
  readonly onInspectResource: (resource: ResourceDto) => void;
  readonly selectedRelationId?: string;
  readonly selectedResourceId?: string;
}) {
  const resource = membership.resource;
  return (
    <div className={`topology-membership${selectedRelationId === membership.relation.relation_id ? " is-relation-selected" : ""}`}>
      {resource === null ? (
        <span className="topology-membership-unresolved" title={membership.resourceId}>
          {membership.resourceId}
        </span>
      ) : (
        <button
          aria-label={`层级资源 ${resource.display_name} ${resource.kind} ${displayEnum(resource.health)} ${displayEnum(resource.freshness)}`}
          className={resourceClass(resource, selectedResourceId === resource.resource_id)}
          onClick={() => onInspectResource(resource)}
          title={resource.display_name}
          type="button"
        >
          <strong>{resource.display_name}</strong>
          <span>{displayEnum(resource.health)}</span>
        </button>
      )}
      <div className="topology-membership-actions">
        <button
          aria-label={`查看 ${resource?.display_name ?? membership.resourceId} 的关系证据 ${membership.relation.kind}`}
          onClick={() => onInspectRelation(membership.relation)}
          type="button"
        >
          证据
        </button>
        {resource !== null && onFocusResource !== undefined ? (
          <button
            aria-label={`将 ${resource.display_name} 设为焦点`}
            onClick={() => onFocusResource(resource.resource_id)}
            type="button"
          >
            设为焦点
          </button>
        ) : null}
      </div>
    </div>
  );
}

function GroupCard({
  expanded,
  group,
  layout,
  onFocusResource,
  onInspectRelation,
  onInspectResource,
  onToggleGroup,
  selectedRelationId,
  selectedResourceId,
  truncated,
}: {
  readonly expanded: boolean;
  readonly group: TopologyChildGroup;
  readonly layout: TopologyHierarchyGroupLayout;
  readonly onFocusResource?: (resourceId: string) => void;
  readonly onInspectRelation: (relation: RelationDto) => void;
  readonly onInspectResource: (resource: ResourceDto) => void;
  readonly onToggleGroup: () => void;
  readonly selectedRelationId?: string;
  readonly selectedResourceId?: string;
  readonly truncated: boolean;
}) {
  const contentId = `topology-hierarchy-group-${layout.id.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
  return (
    <section
      className={`topology-hierarchy-group${expanded ? " is-expanded" : ""}`}
      style={{ height: layout.height, left: layout.x, top: layout.y, width: layout.width }}
    >
      <div className="topology-hierarchy-group-header">
        <div>
          <strong>{groupLabel(group)}</strong>
          <span>{truncated ? "已加载" : "当前已加载"} {group.memberships.length}</span>
        </div>
        <button
          aria-label={`${expanded ? "收起" : "展开"} ${groupLabel(group)} 分组`}
          aria-controls={contentId}
          aria-expanded={expanded}
          onClick={onToggleGroup}
          type="button"
        >
          {expanded ? "收起" : "展开"}
        </button>
      </div>
      <div id={contentId}>
        {expanded ? group.memberships.map((membership, index) => {
          const position = layout.itemPositions[index];
          if (position === undefined) return null;
          return (
            <div
              className="topology-hierarchy-item"
              key={`${membership.resourceId}-${membership.relation.relation_id}`}
              style={{ left: 16, top: position.y - layout.y }}
            >
              <MembershipActions
                membership={membership}
                onFocusResource={onFocusResource}
                onInspectRelation={onInspectRelation}
                onInspectResource={onInspectResource}
                selectedRelationId={selectedRelationId}
                selectedResourceId={selectedResourceId}
              />
            </div>
          );
        }) : null}
      </div>
    </section>
  );
}

export function TopologyHierarchy({
  expandedGroups,
  onFocusResource,
  onInspectRelation,
  onInspectResource,
  onToggleGroup,
  presentation,
  selectedRelationId,
  selectedResourceId,
  truncated,
}: TopologyHierarchyProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const groups = presentation.childGroups.map((group) => {
    const id = groupId(group);
    return {
      expanded: expandedGroups.has(id),
      id,
      totalLoadedCount: group.memberships.length,
      visibleItemCount: group.memberships.length,
    };
  });
  const layout = layoutTopologyHierarchy({
    parentCount: presentation.parentMemberships.length,
    groups,
  });
  const layoutById = new Map(layout.groups.map((entry) => [entry.id, entry]));

  useLayoutEffect(() => {
    const scrollContainer = scrollContainerRef.current;
    if (scrollContainer === null) return;

    const focusCenter = layout.focusRect.x + layout.focusRect.width / 2;
    const maxScrollLeft = Math.max(
      0,
      scrollContainer.scrollWidth - scrollContainer.clientWidth,
    );
    scrollContainer.scrollLeft = Math.min(
      maxScrollLeft,
      Math.max(0, focusCenter - scrollContainer.clientWidth / 2),
    );
  }, [
    layout.focusRect.width,
    layout.focusRect.x,
    presentation.focusResource?.resource_id,
  ]);

  return (
    <section className="topology-hierarchy" aria-labelledby="topology-hierarchy-title">
      <div className="topology-plane-header">
        <div>
          <p>资源层级</p>
          <h2 id="topology-hierarchy-title">所属与包含关系</h2>
        </div>
        <span>包含关系按资源类型分组</span>
      </div>
      <div className="topology-hierarchy-scroll" ref={scrollContainerRef}>
        <div className="topology-hierarchy-canvas" style={{ height: layout.height }}>
          <div className="topology-hierarchy-label topology-hierarchy-label--parent">所属资源</div>
          {presentation.parentMemberships.length === 0 ? (
            <p className="topology-hierarchy-none" style={{ top: layout.parentRegion.y }}>
              当前结果中没有所属资源。
            </p>
          ) : presentation.parentMemberships.map((membership, index) => {
            const position = layout.parentPositions[index];
            if (position === undefined) return null;
            return (
              <div
                className="topology-hierarchy-parent"
                key={`${membership.resourceId}-${membership.relation.relation_id}`}
                style={{
                  height: position.rect.height,
                  left: position.rect.x,
                  top: position.rect.y,
                  width: position.rect.width,
                }}
              >
                <MembershipActions
                  membership={membership}
                  onFocusResource={onFocusResource}
                  onInspectRelation={onInspectRelation}
                  onInspectResource={onInspectResource}
                  selectedRelationId={selectedRelationId}
                  selectedResourceId={selectedResourceId}
                />
              </div>
            );
          })}

          <div className="topology-hierarchy-label topology-hierarchy-label--focus" style={{ top: layout.focusRegion.y - 18 }}>
            当前焦点
          </div>
          {presentation.focusResource === null ? (
            <div className="topology-node topology-node--placeholder" style={layout.focusRect}>
              <span>焦点资源缺失</span>
            </div>
          ) : (
            <button
              aria-label={`层级焦点 ${presentation.focusResource.display_name} ${presentation.focusResource.kind} ${displayEnum(presentation.focusResource.health)} ${displayEnum(presentation.focusResource.freshness)}`}
              className={`topology-node topology-node--health-${presentation.focusResource.health} is-focus${selectedResourceId === presentation.focusResource.resource_id ? " is-selected" : ""}`}
              onClick={() => onInspectResource(presentation.focusResource!)}
              style={{
                height: layout.focusRect.height,
                left: layout.focusRect.x,
                top: layout.focusRect.y,
                width: layout.focusRect.width,
              }}
              type="button"
            >
              <span>{presentation.focusResource.kind}</span>
              <strong>{presentation.focusResource.display_name}</strong>
              <code>{displayEnum(presentation.focusResource.health)} · {displayEnum(presentation.focusResource.freshness)}</code>
            </button>
          )}

          <div className="topology-hierarchy-label topology-hierarchy-label--groups" style={{ top: layout.groupRegion.y - 18 }}>
            包含资源
          </div>
          {presentation.childGroups.length === 0 ? (
            <p className="topology-hierarchy-none" style={{ top: layout.groupRegion.y }}>
              当前结果中没有包含资源。
            </p>
          ) : presentation.childGroups.map((group) => {
            const id = groupId(group);
            const groupLayout = layoutById.get(id);
            if (groupLayout === undefined) return null;
            return (
              <GroupCard
                expanded={expandedGroups.has(id)}
                group={group}
                key={id}
                layout={groupLayout}
                onFocusResource={onFocusResource}
                onInspectRelation={onInspectRelation}
                onInspectResource={onInspectResource}
                onToggleGroup={() => onToggleGroup(id)}
                selectedRelationId={selectedRelationId}
                selectedResourceId={selectedResourceId}
                truncated={truncated}
              />
            );
          })}
        </div>
      </div>
    </section>
  );
}
