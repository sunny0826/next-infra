import type { Ref } from "react";
import type { RouteId, ShellRoute } from "../app/routes";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";
import { ConnectorsPage } from "../features/connectors/ConnectorsPage";
import { InventoryPage } from "../features/inventory/InventoryPage";
import { OverviewPage } from "../features/overview/OverviewPage";
import { ResourceDetailPage } from "../features/resource-detail/ResourceDetailPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { TopologyPage } from "../features/topology/TopologyPage";
import { TimelinePage } from "../features/timeline/TimelinePage";
import { INSPECTOR_ASIDE_ID } from "./InspectorHost";

interface PrimaryCanvasProps {
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  route: ShellRoute;
  detailResourceId: string | null;
  topologyFocusId: string | null;
  queryVersion: number;
  onNavigate: (routeId: RouteId) => void;
  onInspectResource: (resource: ResourceDto) => void;
  onInspectRelation: (relation: RelationDto) => void;
  onCreateRelation: (source: ResourceDto | null) => void;
  onEditRelation: (relation: RelationDto) => void;
  onSelectResource: (resource: ResourceDto) => void;
  onTopologyFocus: (resourceId: string) => void;
  openInspectorButtonRef: Ref<HTMLButtonElement>;
}

export function PrimaryCanvas({
  inspectorOpen,
  onOpenInspector,
  route,
  detailResourceId,
  topologyFocusId,
  queryVersion,
  onNavigate,
  onInspectResource,
  onInspectRelation,
  onCreateRelation,
  onEditRelation,
  onSelectResource,
  onTopologyFocus,
  openInspectorButtonRef,
}: PrimaryCanvasProps) {
  let content;
  if (route.id === "overview") content = <OverviewPage onInspectResource={onInspectResource} onNavigate={onNavigate} queryVersion={queryVersion} />;
  else if (route.id === "inventory") content = detailResourceId ? <ResourceDetailPage resourceId={detailResourceId} queryVersion={queryVersion} /> : <InventoryPage onSelectResource={onSelectResource} queryVersion={queryVersion} />;
  else if (route.id === "topology") content = topologyFocusId ? <TopologyPage focusResourceId={topologyFocusId} onCreateRelation={onCreateRelation} onEditRelation={onEditRelation} onFocusResource={onTopologyFocus} onInspectRelation={onInspectRelation} onInspectResource={onInspectResource} queryVersion={queryVersion} /> : <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">受限关系查询</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>建立或查看资源关系</h2><p>直接创建手工关系，或从资源清单与全局搜索选择资源作为拓扑焦点。</p><button className="shell-control-button" onClick={() => onCreateRelation(null)} type="button">新增关联</button></div></section></>;
  else if (route.id === "timeline") content = <TimelinePage queryVersion={queryVersion} />;
  else if (route.id === "connectors") content = <ConnectorsPage queryVersion={queryVersion} />;
  else if (route.id === "settings") content = <SettingsPage queryVersion={queryVersion} />;
  else content = <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">路由不可用</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>时间线在 Goal 7 前不可用</h2><p>此路由是有意不可用，并非空查询结果。</p></div></section></>;
  return (
    <main className="shell-primary-canvas" id="primary-canvas" tabIndex={-1}>
      <div className="shell-route-page shell-route-page--feature">
        <header className="shell-feature-controls">
          {!inspectorOpen ? (
            <button
              aria-controls={INSPECTOR_ASIDE_ID}
              aria-expanded={inspectorOpen}
              className="shell-control-button"
              onClick={onOpenInspector}
              ref={openInspectorButtonRef}
              type="button"
            >
              打开检查器
            </button>
          ) : null}
        </header>

        {content}
      </div>
    </main>
  );
}
