import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "../../app/AppShell";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { createTopologyHierarchyAdapter } from "../topology-hierarchy/topology-hierarchy-adapter";
import "../../styles/shell.css";

const container = document.getElementById("root");

if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <DesktopAdapterProvider adapter={createTopologyHierarchyAdapter()}>
        <AppShell />
      </DesktopAdapterProvider>
    </StrictMode>,
  );
}
