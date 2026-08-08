import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./topology.css";

interface TopologyPageProps {
  readonly focusResourceId: string;
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
  readonly onFocusResource?: (resourceId: string) => void;
  readonly queryVersion?: number;
}

interface Point { readonly x: number; readonly y: number; }

function activate(event: KeyboardEvent, action: () => void) {
  if (event.key !== "Enter" && event.key !== " ") return;
  event.preventDefault();
  action();
}

function edgeClass(relation: RelationDto): string {
  return `topology-edge topology-edge--${relation.evidence_type}`;
}

function adjacentResourceId(topology: TopologyDto, resourceId: string, key: string): string | null {
  const outgoing = topology.edges
    .filter((edge) => edge.source_resource_id === resourceId)
    .map((edge) => edge.target_resource_id)
    .sort();
  const incoming = topology.edges
    .filter((edge) => edge.target_resource_id === resourceId)
    .map((edge) => edge.source_resource_id)
    .sort();
  if (key === "ArrowRight" || key === "ArrowDown") return outgoing[0] ?? null;
  if (key === "ArrowLeft" || key === "ArrowUp") return incoming[0] ?? null;
  return null;
}

export function TopologyPage({
  focusResourceId,
  onInspectResource,
  onInspectRelation,
  onFocusResource,
  queryVersion = 0,
}: TopologyPageProps) {
  const adapter = useDesktopAdapter();
  const [topology, setTopology] = useState<TopologyDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [bindingTarget, setBindingTarget] = useState("");
  const [bindingPending, setBindingPending] = useState(false);
  const [editingBinding, setEditingBinding] = useState<RelationDto | null>(null);

  useEffect(() => {
    let active = true;
    setTopology(null);
    setError(null);
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

  const positions = useMemo(() => {
    const points = new Map<string, Point>();
    topology?.nodes.forEach((node, index) => {
      points.set(node.resource_id, {
        x: 44 + (index % 4) * 180,
        y: 56 + Math.floor(index / 4) * 116,
      });
    });
    return points;
  }, [topology]);

  const refreshTopology = () => adapter.getTopology({
    focus_resource_id: focusResourceId,
    depth: 1,
    max_nodes: 100,
    max_edges: 200,
  });

  const saveBinding = () => {
    if (bindingTarget.length === 0) return;
    setBindingPending(true);
    const input = {
        source_resource_id: focusResourceId,
        target_resource_id: bindingTarget,
        kind: "infra.depends_on",
      };
    const command = editingBinding?.evidence.type === "configured"
      ? adapter.updateBinding({ binding_id: editingBinding.evidence.binding_id, ...input })
      : adapter.createBinding(input);
    command
      .then(() => refreshTopology())
      .then(setTopology)
      .then(() => setEditingBinding(null))
      .catch(() => setError("无法保存本地绑定。"))
      .finally(() => setBindingPending(false));
  };

  const disableBinding = () => {
    if (editingBinding?.evidence.type !== "configured") return;
    setBindingPending(true);
    adapter
      .disableBinding({ binding_id: editingBinding.evidence.binding_id })
      .then(() => refreshTopology())
      .then(setTopology)
      .then(() => setEditingBinding(null))
      .catch(() => setError("无法禁用本地绑定。"))
      .finally(() => setBindingPending(false));
  };

  if (error !== null) return <section className="topology-state topology-state--error" role="alert">{error}</section>;
  if (topology === null) return <section className="topology-state" aria-busy="true">正在读取焦点拓扑…</section>;

  const canvasHeight = Math.max(320, 100 + Math.ceil(topology.nodes.length / 4) * 116);
  return (
    <div className="topology-page">
      <header className="topology-header">
        <div><p className="topology-eyebrow">证据约束图</p><h1>拓扑</h1><p>检查一个本地资源及其直接、有来源的关系。</p></div>
        <code>{topology.metadata.generated_at}</code>
      </header>

      <div className="topology-toolbar" aria-label="拓扑查询限制">
        <span><small>焦点</small><code>{topology.focus_resource_id}</code></span>
        <span><small>深度</small><strong>{topology.depth}</strong></span>
        <span><small>节点</small><strong>{topology.nodes.length} / 100</strong></span>
        <span><small>边</small><strong>{topology.edges.length} / 200</strong></span>
        <span><small>硬性上限</small><strong>200 / 400</strong></span>
        <span className={topology.truncated ? "topology-truncated" : ""}><small>结果</small><strong>{topology.truncated ? "已截断" : "受限"}</strong></span>
      </div>

      <form className="topology-binding" onSubmit={(event) => { event.preventDefault(); saveBinding(); }}>
        {editingBinding !== null ? <p>正在编辑已配置绑定 <code>{editingBinding.evidence.type === "configured" ? editingBinding.evidence.binding_id : ""}</code></p> : null}
        <label>目标
          <select value={bindingTarget} onChange={(event) => setBindingTarget(event.target.value)}>
            <option value="">选择受限资源</option>
            {topology.nodes.filter((node) => node.resource_id !== focusResourceId).map((node) => <option key={node.resource_id} value={node.resource_id}>{node.display_name}</option>)}
          </select>
        </label>
        <label>关系
          <select disabled><option>infra.depends_on</option></select>
        </label>
        <button disabled={bindingPending || bindingTarget.length === 0} type="submit">{editingBinding === null ? "创建绑定" : "更新绑定"}</button>
        {editingBinding !== null ? <button disabled={bindingPending} onClick={disableBinding} type="button">禁用绑定</button> : null}
        {editingBinding !== null ? <button disabled={bindingPending} onClick={() => setEditingBinding(null)} type="button">取消编辑</button> : null}
      </form>

      <div className="topology-canvas-scroll">
        <div className="topology-canvas" style={{ height: canvasHeight }}>
          <svg aria-label="受限关系边" height={canvasHeight} width="760">
            <defs><marker id="topology-arrow" markerHeight="6" markerWidth="7" orient="auto" refX="6" refY="3"><path d="M0,0 L7,3 L0,6 Z" /></marker></defs>
            {topology.edges.map((relation) => {
              const source = positions.get(relation.source_resource_id);
              const target = positions.get(relation.target_resource_id);
              if (!source || !target) return null;
              const x1 = source.x + 136; const y1 = source.y + 32;
              const x2 = target.x; const y2 = target.y + 32;
              const mainOffset = relation.evidence_type === "configured" ? -2 : 0;
              return (
                <g key={relation.relation_id}>
                  {relation.evidence_type === "configured" ? <path className="topology-edge topology-edge--configured topology-edge--parallel" d={`M${x1},${y1 + 2} L${x2},${y2 + 2}`} /> : null}
                  <path className={edgeClass(relation)} d={`M${x1},${y1 + mainOffset} L${x2},${y2 + mainOffset}`} markerEnd="url(#topology-arrow)" />
                  <path
                    aria-label={`${displayEnum(relation.evidence_type)}关系 ${relation.kind}`}
                    className="topology-edge-hit"
                    d={`M${x1},${y1} L${x2},${y2}`}
                    onClick={() => {
                      onInspectRelation?.(relation);
                      if (relation.evidence.type === "configured") {
                        setEditingBinding(relation);
                        setBindingTarget(relation.target_resource_id);
                      }
                    }}
                    onKeyDown={(event) => activate(event, () => {
                      onInspectRelation?.(relation);
                      if (relation.evidence.type === "configured") {
                        setEditingBinding(relation);
                        setBindingTarget(relation.target_resource_id);
                      }
                    })}
                    role="button"
                    tabIndex={0}
                  />
                  <text className="topology-edge-label" x={(x1 + x2) / 2} y={(y1 + y2) / 2 - 6}>{relation.evidence_type === "configured" && relation.lifecycle === "orphaned" ? "已配置 · 未解析" : displayEnum(relation.evidence_type)}</text>
                </g>
              );
            })}
          </svg>
          {topology.nodes.map((node) => {
            const point = positions.get(node.resource_id)!;
            return (
              <button
                className={`topology-node${node.resource_id === topology.focus_resource_id ? " is-focus" : ""}`}
                id={`topology-node-${node.resource_id}`}
                key={node.resource_id}
                onClick={() => onInspectResource?.(node)}
                onKeyDown={(event) => {
                  const next = adjacentResourceId(topology, node.resource_id, event.key);
                  if (next === null) return;
                  event.preventDefault();
                  document.getElementById(`topology-node-${next}`)?.focus();
                }}
                style={{ left: point.x, top: point.y }}
                type="button"
              >
                <span>{node.kind}</span><strong>{node.display_name}</strong><code>{displayEnum(node.health)} · {displayEnum(node.freshness)}</code>
              </button>
            );
          })}
        </div>
      </div>

      <div className="topology-footer">
        <div className="topology-legend" aria-label="关系证据图例">
          <span className="legend-provider">提供方 · 实线</span>
          <span className="legend-configured">已配置 · 双线</span>
          <span className="legend-inferred">推断 · 虚线</span>
        </div>
        <section className="topology-frontier" aria-labelledby="topology-frontier">
          <div><h2 id="topology-frontier">边界</h2><span>{topology.frontier.length} 个续查点</span></div>
          {topology.frontier.length === 0 ? <p>未返回额外的受限边界。</p> : topology.frontier.map((frontier) => (
            <button key={`${frontier.resource_id}-${frontier.direction}`} onClick={() => onFocusResource?.(frontier.resource_id)} type="button">
              <code>{frontier.resource_id}</code><span>{frontier.direction} · 继续受限查询</span>
            </button>
          ))}
        </section>
      </div>
    </div>
  );
}
