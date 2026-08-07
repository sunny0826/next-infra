import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { OverviewPage } from "./OverviewPage";

afterEach(cleanup);

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
    expect(await screen.findAllByText("已保存事实已过期")).not.toHaveLength(0);
    expect(screen.getByText("不可达")).toBeInTheDocument();
    expect(screen.getAllByText("健康度").length).toBeGreaterThan(0);
    expect(screen.getAllByText("新鲜度").length).toBeGreaterThan(0);
  });

  it("does not invent a critical path from fixture activity", async () => {
    renderPage();
    expect(
      await screen.findByText(/当前没有固定关键路径/),
    ).toBeInTheDocument();
  });

  it("keeps observation timestamps visible", async () => {
    renderPage();
    expect(await screen.findAllByText("2000-01-01T00:00:00Z")).not.toHaveLength(0);
  });
});
