import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { OverviewPage } from "./OverviewPage";

function renderPage() {
  render(
    <DesktopAdapterProvider
      adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}
    >
      <OverviewPage />
    </DesktopAdapterProvider>,
  );
}

describe("OverviewPage", () => {
  it("separates expired facts from resource health and connector failures", async () => {
    renderPage();
    expect(await screen.findAllByText("Saved fact is expired")).not.toHaveLength(0);
    expect(screen.getByText("unreachable")).toBeInTheDocument();
    expect(screen.getAllByText("Health").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Freshness").length).toBeGreaterThan(0);
  });

  it("does not invent a critical path from fixture activity", async () => {
    renderPage();
    expect(
      await screen.findByText(/No critical path is pinned/),
    ).toBeInTheDocument();
  });

  it("keeps observation timestamps visible", async () => {
    renderPage();
    expect(await screen.findAllByText("2000-01-01T00:00:00Z")).not.toHaveLength(0);
  });
});
