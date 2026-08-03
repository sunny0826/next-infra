import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "./app/AppShell";
import { DesktopAdapterProvider } from "./platform/desktop-adapter/DesktopAdapterContext";
import { EmptyDesktopAdapter } from "./platform/desktop-adapter/empty-desktop-adapter";
import { RealDesktopAdapter } from "./platform/desktop-adapter/real-desktop-adapter";
import "./styles/shell.css";

const container = document.getElementById("root");
const desktopAdapter = "__TAURI_INTERNALS__" in window
  ? new RealDesktopAdapter()
  : new EmptyDesktopAdapter();

if (container) {
  createRoot(container).render(
    <StrictMode>
      <DesktopAdapterProvider adapter={desktopAdapter}>
        <AppShell />
      </DesktopAdapterProvider>
    </StrictMode>,
  );
}
