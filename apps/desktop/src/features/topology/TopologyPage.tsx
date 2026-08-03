import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TopologyDto } from "../../generated/query/TopologyDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./topology.css";

interface TopologyPageProps {
  readonly focusResourceId: string;
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
  readonly onFocusResource?: (resourceId: string) => void;
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

export function TopologyPage({
  focusResourceId,
  onInspectResource,
  onInspectRelation,
  onFocusResource,
}: TopologyPageProps) {
  const adapter = useDesktopAdapter();
  const [topology, setTopology] = useState<TopologyDto | null>(null);
  const [error, setError] = useState<string | null>(null);

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
      .catch(() => { if (active) setError("The bounded topology query could not be completed."); });
    return () => { active = false; };
  }, [adapter, focusResourceId]);

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

  if (error !== null) return <section className="topology-state topology-state--error" role="alert">{error}</section>;
  if (topology === null) return <section className="topology-state" aria-busy="true">Reading focus topology…</section>;

  const canvasHeight = Math.max(320, 100 + Math.ceil(topology.nodes.length / 4) * 116);
  return (
    <div className="topology-page">
      <header className="topology-header">
        <div><p className="topology-eyebrow">Evidence-bounded graph</p><h1>Topology</h1><p>Inspect one local resource and its immediate, sourced relations.</p></div>
        <code>{topology.metadata.generated_at}</code>
      </header>

      <div className="topology-toolbar" aria-label="Topology query limits">
        <span><small>Focus</small><code>{topology.focus_resource_id}</code></span>
        <span><small>Depth</small><strong>{topology.depth}</strong></span>
        <span><small>Nodes</small><strong>{topology.nodes.length} / 100</strong></span>
        <span><small>Edges</small><strong>{topology.edges.length} / 200</strong></span>
        <span><small>Hard limit</small><strong>200 / 400</strong></span>
        <span className={topology.truncated ? "topology-truncated" : ""}><small>Result</small><strong>{topology.truncated ? "truncated" : "bounded"}</strong></span>
      </div>

      <div className="topology-canvas-scroll">
        <div className="topology-canvas" style={{ height: canvasHeight }}>
          <svg aria-label="Bounded relation edges" height={canvasHeight} width="760">
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
                    aria-label={`${relation.evidence_type} relation ${relation.kind}`}
                    className="topology-edge-hit"
                    d={`M${x1},${y1} L${x2},${y2}`}
                    onClick={() => onInspectRelation?.(relation)}
                    onKeyDown={(event) => activate(event, () => onInspectRelation?.(relation))}
                    role="button"
                    tabIndex={0}
                  />
                  <text className="topology-edge-label" x={(x1 + x2) / 2} y={(y1 + y2) / 2 - 6}>{relation.evidence_type}</text>
                </g>
              );
            })}
          </svg>
          {topology.nodes.map((node) => {
            const point = positions.get(node.resource_id)!;
            return (
              <button
                className={`topology-node${node.resource_id === topology.focus_resource_id ? " is-focus" : ""}`}
                key={node.resource_id}
                onClick={() => onInspectResource?.(node)}
                style={{ left: point.x, top: point.y }}
                type="button"
              >
                <span>{node.kind}</span><strong>{node.display_name}</strong><code>{node.health} · {node.freshness}</code>
              </button>
            );
          })}
        </div>
      </div>

      <div className="topology-footer">
        <div className="topology-legend" aria-label="Relation evidence legend">
          <span className="legend-provider">provider · solid</span>
          <span className="legend-configured">configured · double</span>
          <span className="legend-inferred">inferred · dashed</span>
        </div>
        <section className="topology-frontier" aria-labelledby="topology-frontier">
          <div><h2 id="topology-frontier">Frontier</h2><span>{topology.frontier.length} continuation points</span></div>
          {topology.frontier.length === 0 ? <p>No additional bounded frontier was returned.</p> : topology.frontier.map((frontier) => (
            <button key={`${frontier.resource_id}-${frontier.direction}`} onClick={() => onFocusResource?.(frontier.resource_id)} type="button">
              <code>{frontier.resource_id}</code><span>{frontier.direction} · continue bounded query</span>
            </button>
          ))}
        </section>
      </div>
    </div>
  );
}
