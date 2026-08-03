import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "../../app/AppShell";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../fixtures/query-fixtures";
import "../../styles/shell.css";

const container = document.getElementById("root");
const adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture());

if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <DesktopAdapterProvider adapter={adapter}>
        <AppShell />
      </DesktopAdapterProvider>
    </StrictMode>,
  );
}
