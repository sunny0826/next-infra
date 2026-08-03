import type { ShellRoute } from "../app/routes";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";
import { ConnectorsPage } from "../features/connectors/ConnectorsPage";
import { InventoryPage } from "../features/inventory/InventoryPage";
import { OverviewPage } from "../features/overview/OverviewPage";
import { ResourceDetailPage } from "../features/resource-detail/ResourceDetailPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { TopologyPage } from "../features/topology/TopologyPage";

interface PrimaryCanvasProps {
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  route: ShellRoute;
  detailResourceId: string | null;
  topologyFocusId: string | null;
  onInspectResource: (resource: ResourceDto) => void;
  onInspectRelation: (relation: RelationDto) => void;
  onSelectResource: (resource: ResourceDto) => void;
  onTopologyFocus: (resourceId: string) => void;
}

export function PrimaryCanvas({ inspectorOpen, onOpenInspector, route, detailResourceId, topologyFocusId, onInspectResource, onInspectRelation, onSelectResource, onTopologyFocus }: PrimaryCanvasProps) {
  let content;
  if (route.id === "overview") content = <OverviewPage onInspectResource={onInspectResource} />;
  else if (route.id === "inventory") content = detailResourceId ? <ResourceDetailPage resourceId={detailResourceId} /> : <InventoryPage onSelectResource={onSelectResource} />;
  else if (route.id === "topology") content = topologyFocusId ? <TopologyPage focusResourceId={topologyFocusId} onFocusResource={onTopologyFocus} onInspectRelation={onInspectRelation} onInspectResource={onInspectResource} /> : <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">Bounded relation query</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>Select a topology focus</h2><p>Choose a resource from Inventory or global search to open a bounded focus.</p></div></section></>;
  else if (route.id === "connectors") content = <ConnectorsPage />;
  else if (route.id === "settings") content = <SettingsPage />;
  else content = <><header className="shell-route-header"><div className="shell-route-title"><p className="shell-eyebrow">Unavailable route</p><h1>{route.label}</h1><p>{route.description}</p></div></header><section className="shell-placeholder"><div><h2>Timeline unavailable until Goal 7</h2><p>This route is intentionally unavailable, not an empty query result.</p></div></section></>;
  return (
    <main className="shell-primary-canvas" id="primary-canvas" tabIndex={-1}>
      <div className="shell-route-page shell-route-page--feature">
        <header className="shell-feature-controls">
          {!inspectorOpen ? (
            <button className="shell-control-button" onClick={onOpenInspector} type="button">
              Open inspector
            </button>
          ) : null}
        </header>

        {content}
      </div>
    </main>
  );
}
