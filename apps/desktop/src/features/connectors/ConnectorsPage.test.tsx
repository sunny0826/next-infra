import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createConnectorCoverageFixtures, createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { ConnectorsPage } from "./ConnectorsPage";

afterEach(cleanup);

class ConnectorAdapter extends MockDesktopAdapter {
  override async listConnectorCoverage() { return { metadata: (await this.searchResources()).metadata, items: [...createConnectorCoverageFixtures()] }; }
}

describe("ConnectorsPage", () => {
  it("separates connector health and declared coverage", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("unreachable")).toBeInTheDocument();
    expect(screen.getByText("fixture.compute")).toBeInTheDocument();
    expect(screen.getByText("supported")).toBeInTheDocument();
  });

  it("reports manual sync without claiming the page refreshed", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    fireEvent.click((await screen.findAllByRole("button", { name: "Manual Sync" }))[0]);
    expect(await screen.findByText(/current page was not refreshed/)).toBeInTheDocument();
  });
});
