import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { InventoryPage } from "./InventoryPage";

afterEach(cleanup);

function renderPage(onSelectResource = vi.fn()) {
  render(
    <DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}>
      <InventoryPage onSelectResource={onSelectResource} />
    </DesktopAdapterProvider>,
  );
  return onSelectResource;
}

describe("InventoryPage", () => {
  it("renders health freshness lifecycle and observed time as separate columns", async () => {
    renderPage();
    expect(await screen.findByText("Fixture Compute Alpha")).toBeInTheDocument();
    for (const heading of ["Health", "Freshness", "Lifecycle", "Observed"]) {
      expect(screen.getByRole("columnheader", { name: heading })).toBeInTheDocument();
    }
  });

  it("filters to attention resources without editing opaque cursors", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    fireEvent.click(screen.getByRole("button", { name: "Attention only" }));
    expect(screen.queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
    expect(screen.getByText("Fixture Database Beta")).toBeInTheDocument();
  });

  it("selects a row with Enter", async () => {
    const onSelect = renderPage();
    const row = (await screen.findByText("Fixture Compute Alpha")).closest("tr");
    fireEvent.keyDown(row!, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
