import { useEffect, useRef, useState } from "react";

import { ContextBar } from "../ui/ContextBar";
import { InspectorHost } from "../ui/InspectorHost";
import { Navigation } from "../ui/Navigation";
import { PrimaryCanvas } from "../ui/PrimaryCanvas";
import { RuntimeBar } from "../ui/RuntimeBar";
import { getRoute, type RouteId } from "./routes";

export function AppShell() {
  const [activeRoute, setActiveRoute] = useState<RouteId>("overview");
  const [inspectorOpen, setInspectorOpen] = useState(() => {
    if (typeof window.matchMedia !== "function") return true;
    return !window.matchMedia("(max-width: 1180px)").matches;
  });
  const searchInputRef = useRef<HTMLInputElement>(null);
  const route = getRoute(activeRoute);

  useEffect(() => {
    function focusSearch(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      searchInputRef.current?.focus();
    }

    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
  }, []);

  function navigate(routeId: RouteId) {
    setActiveRoute(routeId);
    if (routeId === "settings") setInspectorOpen(false);
  }

  return (
    <div className={`app-shell${inspectorOpen ? "" : " inspector-closed"}`}>
      <Navigation activeRoute={activeRoute} onNavigate={navigate} />
      <ContextBar contextLabel={route.label} routeId={route.id} searchInputRef={searchInputRef} />
      <PrimaryCanvas
        inspectorOpen={inspectorOpen}
        onOpenInspector={() => setInspectorOpen(true)}
        route={route}
      />
      <InspectorHost
        onClose={() => setInspectorOpen(false)}
        open={inspectorOpen}
        routeLabel={route.label}
      />
      <RuntimeBar />
    </div>
  );
}
