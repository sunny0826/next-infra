import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "../../app/AppShell";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { createGitHubGoal5Adapter } from "../fixtures/github-goal5-adapter";
import "../../styles/shell.css";

const container = document.getElementById("root");

if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <DesktopAdapterProvider adapter={createGitHubGoal5Adapter()}>
        <AppShell />
      </DesktopAdapterProvider>
    </StrictMode>,
  );
}
