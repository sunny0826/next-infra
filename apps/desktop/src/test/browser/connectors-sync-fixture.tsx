import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { ConnectorsPage } from "../../features/connectors/ConnectorsPage";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createSyncRunSnapshotFixture } from "../fixtures/sync-run-fixture";
import "../../styles/shell.css";
import "./connectors-sync-fixture.css";

/**
 * Browser-only preview of the Connectors SyncRun provenance display.
 * All data is synthetic fixture content (fixture-* ids, fixed timestamps).
 */

const container = document.getElementById("root");
const adapter = new MockDesktopAdapter(createSyncRunSnapshotFixture());

if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <DesktopAdapterProvider adapter={adapter}>
        <div className="connectors-preview">
          <ConnectorsPage queryVersion={0} />
        </div>
      </DesktopAdapterProvider>
    </StrictMode>,
  );
}
