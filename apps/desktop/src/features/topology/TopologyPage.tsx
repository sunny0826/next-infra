import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import {
  layoutTopology,
  parallelOffset,
  relationCurve,
  relationLabelPoint,
  TOPOLOGY_CANVAS_WIDTH,
} from "./topology-layout";

import "./topology.css";

interface TopologyPageProps {
  readonly focusResourceId: string;
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
  readonly onCreateRelation?: (source: ResourceDto) => void;
  readonly onEditRelation?: (relation: RelationDto) => void;
  readonly onFocusResource?: (resourceId: string) => void;
  readonly queryVersion?: number;
}

type TopologyRenderNode =
  | { readonly type: "resource"; readonly resource: ResourceDto }
  | { readonly type: "placeholder"; readonly resource_id: string };

type TopologySelection =
  | { readonly type: "resource"; readonly id: string }
  | { readonly type: "relation"; readonly id: string };

function activate(event: KeyboardEvent, action: () => void) {
  if (event.key !== "Enter" && event.key !== " ") return;
  event.preventDefault();
  action();
}

function edgeLabel(relation: RelationDto): string {
  const evidence = relation.evidence_type === "configured"
    ? "人工声明"
    : displayEnum(relation.evidence_type);
  const unresolved = relation.lifecycle === "orphaned" ? " · 未解析" : "";
  return `${relation.kind} · ${evidence}${unresolved}`;
}

function renderNodes(topology: TopologyDto, relations: readonly RelationDto[]): TopologyRenderNode[] {
  const resourceIds = new Set(topology.nodes.map((node) => node.resource_id));
  const placeholderIds = new Set<string>();
  relations.forEach((edge) => {
    if (!resourceIds.has(edge.source_resource_id)) placeholderIds.add(edge.source_resource_id);
    if (!resourceIds.has(edge.target_resource_id)) placeholderIds.add(edge.target_resource_id);
  });
  return [
    ...topology.nodes.map((resource) => ({ type: "resource" as const, resource })),
    ...[...placeholderIds].sort().map((resource_id) => ({ type: "placeholder" as const, resource_id })),
  ];
}

function adjacentResourceId(relations: readonly RelationDto[], resourceId: string, key: string): string | null {
  const outgoing = relations
    .filter((edge) => edge.source_resource_id === resourceId)
    .map((edge) => edge.target_resource_id)
    .sort();
  const incoming = relations
    .filter((edge) => edge.target_resource_id === resourceId)
    .map((edge) => edge.source_resource_id)
    .sort();
  if (key === "ArrowRight" || key === "ArrowDown") return outgoing[0] ?? null;
  if (key === "ArrowLeft" || key === "ArrowUp") return incoming[0] ?? null;
  return null;
}

function relationOffsets(relations: readonly RelationDto[]): ReadonlyMap<string, number> {
  const grouped = new Map<string, RelationDto[]>();
  relations.forEach((relation) => {
    const key = [relation.source_resource_id, relation.target_resource_id].sort().join("\u0000");
    grouped.set(key, [...(grouped.get(key) ?? []), relation]);
  });
  const offsets = new Map<string, number>();
  grouped.forEach((group) => {
    group.sort((left, right) => left.relation_id.localeCompare(right.relation_id));
    group.forEach((relation, index) => {
      offsets.set(relation.relation_id, parallelOffset(index, group.length));
    });
  });
  return offsets;
}

export function TopologyPage({
  focusResourceId,
  onInspectResource,
  onInspectRelation,
  onCreateRelation,
  onEditRelation,
  onFocusResource,
  queryVersion = 0,
}: TopologyPageProps) {
  const adapter = useDesktopAdapter();
  const [topology, setTopology] = useState<TopologyDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selection, setSelection] = useState<TopologySelection | null>(null);

  useEffect(() => {
    let active = true;
    setTopology(null);
    setError(null);
    setSelection(null);
    adapter
      .getTopology({
        focus_resource_id: focusResourceId,
        depth: 1,
        max_nodes: 100,
        max_edges: 200,
      })
      .then((result) => { if (active) setTopology(result); })
      .catch(() => { if (active) setError("无法完成受限拓扑查询。"); });
    return () => { active = false; };
  }, [adapter, focusResourceId, queryVersion]);

  const visibleEdges = useMemo(
    () => topology?.edges.filter((relation) => !(
      relation.evidence_type === "configured" && relation.lifecycle === "tombstoned"
    )) ?? [],
    [topology],
  );
  const nodes = useMemo(
    () => (topology === null ? [] : renderNodes(topology, visibleEdges)),
    [topology, visibleEdges],
  );
  const layout = useMemo(() => {
    const resourceIds = nodes.map((node) => node.type === "resource"
      ? node.resource.resource_id
      : node.resource_id);
    return layoutTopology(
      resourceIds,
      topology?.focus_resource_id ?? focusResourceId,
      visibleEdges,
    );
  }, [focusResourceId, nodes, topology, visibleEdges]);
  const offsets = useMemo(
    () => relationOffsets(visibleEdges),
    [visibleEdges],
  );

  const focusResource = topology?.nodes.find(
    (resource) => resource.resource_id === topology.focus_resource_id,
  );
  const selectedRelation = selection?.type === "relation"
    ? visibleEdges.find((relation) => relation.relation_id === selection.id)
    : undefined;
  const selectedResource = selection?.type === "resource"
    ? topology?.nodes.find((resource) => resource.resource_id === selection.id)
    : undefined;

  const focusAdjacentNode = (resourceId: string, event: KeyboardEvent) => {
    const next = topology === null ? null : adjacentResourceId(visibleEdges, resourceId, event.key);
    if (next === null) return;
    event.preventDefault();
    document.getElementById(`topology-node-${next}`)?.focus();
  };

  if (error !== null) return <section className="topology-state topology-state--error" role="alert">{error}</section>;
  if (topology === null) return <section className="topology-state" aria-busy="true">正在读取焦点拓扑…</section>;

  const canvasHeight = layout.height + Math.max(0, topology.frontier.length - 1) * 32;
  const laneCounts = [...layout.nodes.values()].reduce(
    (counts, node) => ({ ...counts, [node.lane]: counts[node.lane] + 1 }),
    { incoming: 0, focus: 0, outgoing: 0, context: 0 },
  );
  const contextLabelY = Math.min(
    ...[...layout.nodes.values()]
      .filter((node) => node.lane === "context")
      .map((node) => node.y - 22),
  );
  const isRelationSelected = (relation: RelationDto) =>
    selection?.type === "relation" && selection.id === relation.relation_id;
  const isRelationDimmed = (relation: RelationDto) => {
    if (selection === null) return false;
    if (selection.type === "relation") return selection.id !== relation.relation_id;
    return relation.source_resource_id !== selection.id && relation.target_resource_id !== selection.id;
  };
  const isNodeSelected = (resourceId: string) => {
    if (selection?.type === "resource") return selection.id === resourceId;
    if (selectedRelation === undefined) return false;
    return selectedRelation.source_resource_id === resourceId
      || selectedRelation.target_resource_id === resourceId;
  };
  const isNodeDimmed = (resourceId: string) => {
    if (selection === null) return false;
    if (selection.type === "resource") return selection.id !== resourceId;
    return !isNodeSelected(resourceId);
  };

  return (
    <div className="topology-page">
      <header className="topology-header">
        <div>
          <p className="topology-eyebrow">证据约束图</p>
          <h1>拓扑</h1>
          <p>沿上游、焦点与下游核实直接关系及其证据来源。</p>
        </div>
        <code>{topology.metadata.generated_at}</code>
      </header>

      <div className="topology-toolbar" aria-label="拓扑查询限制">
        <span className="topology-toolbar-focus"><small>焦点</small><code>{topology.focus_resource_id}</code></span>
        <span><small>深度</small><strong>{topology.depth}</strong></span>
        <span><small>节点</small><strong>{topology.nodes.length} / 100</strong></span>
        <span><small>边</small><strong>{visibleEdges.length} / 200</strong></span>
        <span><small>硬性上限</small><strong>200 / 400</strong></span>
        <span className={topology.truncated ? "topology-truncated" : ""}><small>结果</small><strong>{topology.truncated ? "已截断" : "受限"}</strong></span>
        <button
          className="topology-create-relation"
          disabled={focusResource === undefined || onCreateRelation === undefined}
          onClick={() => {
            if (focusResource !== undefined) onCreateRelation?.(focusResource);
          }}
          type="button"
        >
          新增关联
        </button>
      </div>

      <div className="topology-canvas-scroll">
        <div className="topology-canvas" style={{ height: canvasHeight }}>
          <div className="topology-lane topology-lane--incoming" aria-hidden="true" />
          <div className="topology-lane topology-lane--focus" aria-hidden="true" />
          <div className="topology-lane topology-lane--outgoing" aria-hidden="true" />
          <div className="topology-lane-label topology-lane-label--incoming">上游来源 <code>{laneCounts.incoming}</code></div>
          <div className="topology-lane-label topology-lane-label--focus">当前焦点 <code>{laneCounts.focus}</code></div>
          <div className="topology-lane-label topology-lane-label--outgoing">下游目标 <code>{laneCounts.outgoing}</code></div>

          <div className="topology-selection-bar" aria-live="polite">
            {selectedRelation !== undefined ? (
              <>
                <span>已选择关系</span>
                <strong>{edgeLabel(selectedRelation)}</strong>
                {selectedRelation.evidence_type === "configured" && onEditRelation !== undefined ? (
                  <button
                    aria-label={`编辑关联 ${selectedRelation.kind}`}
                    onClick={() => onEditRelation(selectedRelation)}
                    type="button"
                  >
                    编辑关联
                  </button>
                ) : <code>只读证据</code>}
              </>
            ) : selectedResource !== undefined ? (
              <><span>已选择资源</span><strong>{selectedResource.display_name}</strong><code>{selectedResource.kind}</code></>
            ) : (
              <><span>浏览提示</span><strong>选择节点或关系查看证据</strong><code>方向键可沿关系移动</code></>
            )}
          </div>

          <svg
            aria-label="受限关系边"
            height={canvasHeight}
            viewBox={`0 0 ${TOPOLOGY_CANVAS_WIDTH} ${canvasHeight}`}
            width={TOPOLOGY_CANVAS_WIDTH}
          >
            <defs>
              <marker id="topology-arrow" markerHeight="6" markerWidth="7" orient="auto" refX="6" refY="3">
                <path d="M0,0 L7,3 L0,6 Z" />
              </marker>
            </defs>
            {visibleEdges.map((relation) => {
              const source = layout.nodes.get(relation.source_resource_id);
              const target = layout.nodes.get(relation.target_resource_id);
              if (source === undefined || target === undefined) return null;
              const offset = offsets.get(relation.relation_id) ?? 0;
              const label = relationLabelPoint(source, target, offset);
              const selected = isRelationSelected(relation);
              const dimmed = isRelationDimmed(relation);
              const edgeStateClass = `${selected ? " is-selected" : ""}${dimmed ? " is-dimmed" : ""}`;
              return (
                <g className="topology-edge-group" key={relation.relation_id}>
                  {relation.evidence_type === "configured" ? (
                    <path
                      className={`topology-edge topology-edge--configured topology-edge--parallel${edgeStateClass}`}
                      d={relationCurve(source, target, offset + 4)}
                    />
                  ) : null}
                  <path
                    className={`topology-edge topology-edge--${relation.evidence_type}${edgeStateClass}`}
                    d={relationCurve(source, target, offset)}
                    markerEnd="url(#topology-arrow)"
                  />
                  <path
                    aria-label={`${displayEnum(relation.evidence_type)}关系 ${relation.kind}`}
                    className="topology-edge-hit"
                    d={relationCurve(source, target, offset)}
                    onClick={() => {
                      setSelection({ type: "relation", id: relation.relation_id });
                      onInspectRelation?.(relation);
                    }}
                    onKeyDown={(event) => activate(event, () => {
                      setSelection({ type: "relation", id: relation.relation_id });
                      onInspectRelation?.(relation);
                    })}
                    role="button"
                    tabIndex={0}
                  />
                  <text
                    className={`topology-edge-label${selected ? " is-selected" : ""}${dimmed ? " is-dimmed" : ""}`}
                    x={label.x}
                    y={label.y}
                  >
                    {edgeLabel(relation)}
                  </text>
                </g>
              );
            })}
          </svg>

          {nodes.map((node) => {
            const resourceId = node.type === "resource" ? node.resource.resource_id : node.resource_id;
            const point = layout.nodes.get(resourceId);
            if (point === undefined) return null;
            if (node.type === "placeholder") {
              return (
                <div
                  aria-label={`未解析资源 ${resourceId}`}
                  className={`topology-node topology-node--placeholder${isNodeSelected(resourceId) ? " is-selected" : ""}${isNodeDimmed(resourceId) ? " is-dimmed" : ""}`}
                  data-resource-id={resourceId}
                  id={`topology-node-${resourceId}`}
                  key={resourceId}
                  onKeyDown={(event) => focusAdjacentNode(resourceId, event)}
                  style={{ left: point.x, top: point.y }}
                  tabIndex={-1}
                >
                  <span>未解析资源</span>
                  <strong>{resourceId}</strong>
                  <code>端点缺失 · 证据保留</code>
                </div>
              );
            }
            const resource = node.resource;
            return (
              <button
                className={`topology-node topology-node--health-${resource.health}${resource.resource_id === topology.focus_resource_id ? " is-focus" : ""}${isNodeSelected(resourceId) ? " is-selected" : ""}${isNodeDimmed(resourceId) ? " is-dimmed" : ""}`}
                id={`topology-node-${resource.resource_id}`}
                key={resource.resource_id}
                onClick={() => {
                  setSelection({ type: "resource", id: resource.resource_id });
                  onInspectResource?.(resource);
                }}
                onKeyDown={(event) => focusAdjacentNode(resource.resource_id, event)}
                style={{ left: point.x, top: point.y }}
                type="button"
              >
                <span>{resource.kind}</span>
                <strong>{resource.display_name}</strong>
                <code>{displayEnum(resource.health)} · {displayEnum(resource.freshness)}</code>
              </button>
            );
          })}

          {laneCounts.context > 0 ? (
            <div className="topology-context-label" style={{ top: contextLabelY }}>
              相关上下文 <code>{laneCounts.context}</code>
            </div>
          ) : null}

          <div className="topology-legend" aria-label="关系证据图例">
            <span className="legend-provider">提供方 · 实线</span>
            <span className="legend-configured">已配置 · 双线</span>
            <span className="legend-inferred">推断 · 虚线</span>
          </div>
          <section className="topology-frontier" aria-labelledby="topology-frontier">
            <div>
              <h2 id="topology-frontier">边界</h2>
              <span>{topology.frontier.length} 个续查点</span>
            </div>
            {topology.frontier.length === 0 ? <p>当前深度没有额外续查点。</p> : topology.frontier.map((frontier) => (
              <button key={`${frontier.resource_id}-${frontier.direction}`} onClick={() => onFocusResource?.(frontier.resource_id)} type="button">
                <code>{frontier.resource_id}</code>
                <span>{frontier.direction} · 继续受限查询</span>
              </button>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}
