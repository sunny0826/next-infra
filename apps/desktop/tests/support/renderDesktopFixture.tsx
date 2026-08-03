import { render } from "@testing-library/react";

import { AppShell } from "../../src/app/AppShell";
import { DesktopAdapterProvider } from "../../src/platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../src/platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../src/test/fixtures/query-fixtures";

export function renderDesktopFixture() {
  return render(
    <DesktopAdapterProvider
      adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}
    >
      <AppShell />
    </DesktopAdapterProvider>,
  );
}
