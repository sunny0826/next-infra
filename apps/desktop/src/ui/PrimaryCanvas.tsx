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
  onSelectResource: (resource: ResourceDto) => void;
  onTopologyFocus: (resourceId: string) => void;
}

export function PrimaryCanvas({ inspectorOpen, onOpenInspector, route, detailResourceId, topologyFocusId, queryVersion, onNavigate, onInspectResource, onInspectRelation, onSelectResource, onTopologyFocus }: PrimaryCanvasProps) {
  let content;
  if (route.id === "overview") content = <OverviewPage onInspectResource={onInspectResource} onNavigate={onNavigate} queryVersion={queryVersion} />;
  else if (route.id === "inventory") content = detailResourceId ? <ResourceDetailPage resourceId={detailResourceId} queryVersion={queryVersion} /> : <InventoryPage onSelectResource={onSelectResource} queryVersion={queryVersion} />;
  else if (route.id === "topology") content = topologyFocusId ? <TopologyPage focusResourceId={topologyFocusId} onFocusResource={onTopologyFocus} onInspectRelation={onInspectRelation} onInspectResource={onInspectResource} queryVersion={queryVersion} /> : <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">受限关系查询</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>选择拓扑焦点</h2><p>从资源清单或全局搜索中选择资源，以打开受限焦点视图。</p></div></section></>;
  else if (route.id === "timeline") content = <TimelinePage queryVersion={queryVersion} />;
  else if (route.id === "connectors") content = <ConnectorsPage queryVersion={queryVersion} />;
  else if (route.id === "settings") content = <SettingsPage queryVersion={queryVersion} />;
  else content = <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">路由不可用</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>时间线在 Goal 7 前不可用</h2><p>此路由是有意不可用，并非空查询结果。</p></div></section></>;
  return (
    <main className="shell-primary-canvas" id="primary-canvas" tabIndex={-1}>
      <div className="shell-route-page shell-route-page--feature">
        <header className="shell-feature-controls">
          {!inspectorOpen ? (
            <button className="shell-control-button" onClick={onOpenInspector} type="button">
              打开检查器
            </button>
          ) : null}
        </header>

        {content}
      </div>
    </main>
  );
}
