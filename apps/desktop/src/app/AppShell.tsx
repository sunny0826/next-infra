import { useEffect, useRef, useState } from "react";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";
import { useDesktopAdapter } from "../platform/desktop-adapter/DesktopAdapterContext";
import type { Unsubscribe } from "../platform/desktop-adapter/desktop-adapter";

import { ContextBar } from "../ui/ContextBar";
import { InspectorHost, type InspectorSelection } from "../ui/InspectorHost";
import { Navigation } from "../ui/Navigation";
import { PrimaryCanvas } from "../ui/PrimaryCanvas";
import { RuntimeBar } from "../ui/RuntimeBar";
import { getRoute, type RouteId } from "./routes";

interface RouteShellState {
  readonly selection: InspectorSelection;
  readonly detailResourceId: string | null;
  readonly topologyFocusId: string | null;
}

type RouteShellStateMap = Record<RouteId, RouteShellState>;

function createRouteShellState(): RouteShellStateMap {
  const empty = (): RouteShellState => ({
    selection: null,
    detailResourceId: null,
    topologyFocusId: null,
  });

  return {
    overview: empty(),
    inventory: empty(),
    topology: empty(),
    timeline: empty(),
    connectors: empty(),
    settings: empty(),
  };
}

export function AppShell() {
  const adapter = useDesktopAdapter();
  const [activeRoute, setActiveRoute] = useState<RouteId>("overview");
  const [inspectorOpen, setInspectorOpen] = useState(() => {
    if (typeof window.matchMedia !== "function") return true;
    return !window.matchMedia("(max-width: 1180px)").matches;
  });
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [searchValue, setSearchValue] = useState("");
  const [searchResults, setSearchResults] = useState<readonly ResourceDto[]>([]);
  const [routeState, setRouteState] = useState<RouteShellStateMap>(createRouteShellState);
  const [queryVersion, setQueryVersion] = useState(0);
  const route = getRoute(activeRoute);
  const currentRouteState = routeState[activeRoute];

  useEffect(() => {
    function focusSearch(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      searchInputRef.current?.focus();
    }

    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
  }, []);

  useEffect(() => {
    if (searchValue.trim() === "") { setSearchResults([]); return; }
    let active = true;
    adapter.searchResources({ query: searchValue, limit: 6 }).then((page) => { if (active) setSearchResults(page.items); }).catch(() => { if (active) setSearchResults([]); });
    return () => { active = false; };
  }, [adapter, searchValue]);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: Unsubscribe | null = null;
    const onInvalidation = () => setQueryVersion((version) => version + 1);

    adapter.subscribeInvalidations(onInvalidation).then((value) => {
      if (cancelled) {
        value();
        return;
      }
      unsubscribe = value;
    }).catch(() => undefined);
    const restore = () => setQueryVersion((version) => version + 1);
    window.addEventListener("focus", restore);
    return () => {
      cancelled = true;
      unsubscribe?.();
      window.removeEventListener("focus", restore);
    };
  }, [adapter]);

  function updateRouteState(routeId: RouteId, patch: Partial<RouteShellState>) {
    setRouteState((current) => ({
      ...current,
      [routeId]: { ...current[routeId], ...patch },
    }));
  }

  function navigate(routeId: RouteId) {
    setActiveRoute(routeId);
    updateRouteState(routeId, { selection: null, detailResourceId: null });
    if (routeId === "settings") {
      setInspectorOpen(false);
    }
  }

  function inspectResource(resource: ResourceDto, routeId = activeRoute) {
    updateRouteState(routeId, { selection: { type: "resource", resource } });
    setInspectorOpen(true);
  }

  function inspectRelation(relation: RelationDto, routeId = activeRoute) {
    updateRouteState(routeId, { selection: { type: "relation", relation } });
    setInspectorOpen(true);
  }

  function rememberTopologyFocus(resourceId: string) {
    updateRouteState("topology", { topologyFocusId: resourceId });
  }

  function selectInventoryResource(resource: ResourceDto) {
    updateRouteState("inventory", {
      selection: { type: "resource", resource },
      detailResourceId: resource.resource_id,
      topologyFocusId: resource.resource_id,
    });
    rememberTopologyFocus(resource.resource_id);
    setInspectorOpen(true);
  }

  function selectSearchResult(resource: ResourceDto) {
    setSearchValue("");
    setSearchResults([]);
    setActiveRoute("inventory");
    selectInventoryResource(resource);
  }

  function clearSearch() {
    setSearchValue("");
    setSearchResults([]);
  }

  function focusTopology(resourceId: string) {
    updateRouteState("topology", { topologyFocusId: resourceId });
  }

  return (
    <div className={`app-shell${inspectorOpen ? "" : " inspector-closed"}`}>
      <Navigation activeRoute={activeRoute} onNavigate={navigate} />
      <ContextBar
        contextLabel={route.label}
        onSearchChange={setSearchValue}
        onSearchClear={clearSearch}
        onSearchSelect={selectSearchResult}
        routeId={route.id}
        searchInputRef={searchInputRef}
        searchResults={searchResults}
        searchValue={searchValue}
      />
      <PrimaryCanvas
        key={route.id}
        detailResourceId={currentRouteState.detailResourceId}
        inspectorOpen={inspectorOpen}
        onInspectRelation={inspectRelation}
        onInspectResource={inspectResource}
        onNavigate={navigate}
        onOpenInspector={() => setInspectorOpen(true)}
        onSelectResource={selectInventoryResource}
        onTopologyFocus={focusTopology}
        queryVersion={queryVersion}
        route={route}
        topologyFocusId={currentRouteState.topologyFocusId}
      />
      <InspectorHost
        onClose={() => setInspectorOpen(false)}
        open={inspectorOpen}
        routeLabel={route.label}
        selection={currentRouteState.selection}
      />
      <RuntimeBar />
    </div>
  );
}
