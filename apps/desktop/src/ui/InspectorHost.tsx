import { useEffect, useRef, useState, type Ref } from "react";
import { Icon } from "./Icon";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";
import { EvidenceSpine } from "../features/evidence/EvidenceSpine";
import { useDesktopAdapter } from "../platform/desktop-adapter/DesktopAdapterContext";
import { displayEnum } from "../i18n";

/** Shared by the 打开检查器/关闭检查器 toggle buttons via aria-controls. */
export const INSPECTOR_ASIDE_ID = "evidence-inspector";

export type InspectorSelection =
  | { readonly type: "resource"; readonly resource: ResourceDto }
  | { readonly type: "relation"; readonly relation: RelationDto }
  | null;

interface InspectorHostProps {
  onClose: () => void;
  onCreateRelation: (source: ResourceDto | null) => void;
  onEditRelation: (relation: RelationDto) => void;
  open: boolean;
  routeLabel: string;
  selection: InspectorSelection;
  /** Focus target when the inspector opens; owned by AppShell. */
  asideHeadRef: Ref<HTMLDivElement>;
}

interface RelationEvidenceState {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

function linksThePair(
  candidate: RelationDto,
  sourceId: string,
  targetId: string,
): boolean {
  return (
    (candidate.source_resource_id === sourceId &&
      candidate.target_resource_id === targetId) ||
    (candidate.source_resource_id === targetId &&
      candidate.target_resource_id === sourceId)
  );
}

/**
 * Resolves both endpoints of a selected relation and renders the full evidence
 * spine (source fact → evidence path → target fact). The selected relation is
 * always included even when the endpoint details only carry a partial edge
 * list; loading stays muted and resolution failures degrade to a calm message.
 */
function RelationEvidenceSpine({
  onEditRelation,
  relation,
}: {
  readonly onEditRelation: (relation: RelationDto) => void;
  readonly relation: RelationDto;
}) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<RelationEvidenceState | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    setState(null);
    setFailed(false);
    Promise.all([
      adapter.getResource({
        resource_id: relation.source_resource_id,
        include: ["relations"],
      }),
      adapter.getResource({
        resource_id: relation.target_resource_id,
        include: ["relations"],
      }),
    ])
      .then(([sourceDetail, targetDetail]) => {
        if (!active) return;
        const byId = new Map<string, RelationDto>([
          [relation.relation_id, relation],
        ]);
        for (const candidate of [
          ...sourceDetail.relations,
          ...targetDetail.relations,
        ]) {
          if (
            linksThePair(
              candidate,
              relation.source_resource_id,
              relation.target_resource_id,
            )
          ) {
            byId.set(candidate.relation_id, candidate);
          }
        }
        setState({
          source: sourceDetail.resource,
          target: targetDetail.resource,
          relations: [...byId.values()],
        });
      })
      .catch(() => {
        if (active) setFailed(true);
      });
    return () => {
      active = false;
    };
  }, [adapter, relation]);

  if (failed) {
    return (
      <p className="shell-inspector-callout">
        无法加载此关系的端点事实，请稍后重试。
      </p>
    );
  }
  if (state === null) {
    return <p className="shell-inspector-subtitle">正在读取端点事实…</p>;
  }
  return (
    <>
      <EvidenceSpine
        relations={state.relations}
        source={state.source}
        target={state.target}
      />
      {relation.evidence.type === "configured" ? (
        <button
          className="shell-control-button"
          onClick={() => onEditRelation(relation)}
          type="button"
        >
          编辑关联
        </button>
      ) : null}
    </>
  );
}

export function InspectorHost({
  onClose,
  onCreateRelation,
  onEditRelation,
  open,
  routeLabel,
  selection,
  asideHeadRef,
}: InspectorHostProps) {
  return (
    <aside
      aria-label="证据检查器"
      className="shell-inspector"
      hidden={!open}
      id={INSPECTOR_ASIDE_ID}
    >
      <div className="shell-inspector-head" ref={asideHeadRef} tabIndex={-1}>
        <h2>证据链</h2>
        <button
          aria-controls={INSPECTOR_ASIDE_ID}
          aria-expanded={open}
          aria-label="关闭检查器"
          className="shell-icon-button"
          onClick={onClose}
          type="button"
        >
          <Icon name="close" />
        </button>
      </div>

      <div className="shell-inspector-body">
        <p className="shell-inspector-kicker">{routeLabel} 上下文</p>
        {selection === null ? (
          <>
            <p className="shell-inspector-type">未选择</p>
            <h3>证据链</h3>
            <p className="shell-inspector-subtitle">请选择资源或关系</p>
          </>
        ) : null}

        {selection?.type === "resource" ? (
          <>
            <p className="shell-inspector-type">资源</p>
            <h3>{selection.resource.display_name}</h3>
            <code>{selection.resource.resource_id}</code>
            <dl className="shell-inspector-facts">
              <dt>连接</dt><dd><code>{selection.resource.connection_id}</code></dd>
              <dt>范围</dt><dd>{selection.resource.scope}</dd>
            </dl>
            <h4>当前事实</h4>
            <dl className="shell-inspector-facts">
              <dt>健康度</dt><dd>{displayEnum(selection.resource.health)}</dd>
              <dt>新鲜度</dt><dd>{displayEnum(selection.resource.freshness)}</dd>
              <dt>生命周期</dt><dd>{displayEnum(selection.resource.lifecycle)}</dd>
              <dt>观测时间</dt><dd><time dateTime={selection.resource.observed_at}>{selection.resource.observed_at}</time></dd>
            </dl>
            <p className="shell-inspector-callout">
              ResourceDto 仅提供当前事实；此选择中不包含关系来源。
            </p>
            <button
              className="shell-control-button"
              onClick={() => onCreateRelation(selection.resource)}
              type="button"
            >
              从此资源建立关联
            </button>
          </>
        ) : null}

        {selection?.type === "relation" ? (
          <>
            <p className="shell-inspector-type">关系</p>
            <RelationEvidenceSpine
              onEditRelation={onEditRelation}
              relation={selection.relation}
            />
          </>
        ) : null}
      </div>
    </aside>
  );
}
