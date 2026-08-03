import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { GetTopologyInput } from "../../platform/desktop-adapter/desktop-adapter";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { TopologyPage } from "./TopologyPage";

afterEach(cleanup);

class TrackingAdapter extends MockDesktopAdapter {
  request: GetTopologyInput | null = null;
  override async getTopology(input: GetTopologyInput) { this.request = input; return super.getTopology(input); }
}

describe("TopologyPage", () => {
  it("requests the frozen bounded defaults and exposes hard limits", async () => {
    const adapter = new TrackingAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    render(<DesktopAdapterProvider adapter={adapter}><TopologyPage focusResourceId="fixture-resource-alpha" /></DesktopAdapterProvider>);
    expect(await screen.findByText("200 / 400")).toBeInTheDocument();
    expect(adapter.request).toEqual({ focus_resource_id: "fixture-resource-alpha", depth: 1, max_nodes: 100, max_edges: 200 });
    expect(screen.queryByText(/load all/i)).not.toBeInTheDocument();
  });

  it("distinguishes every evidence type with text", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" /></DesktopAdapterProvider>);
    expect(await screen.findByText("provider · solid")).toBeInTheDocument();
    expect(screen.getByText("configured · double")).toBeInTheDocument();
    expect(screen.getByText("inferred · dashed")).toBeInTheDocument();
  });

  it("selects a node for the inspector", async () => {
    const onInspect = vi.fn();
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" onInspectResource={onInspect} /></DesktopAdapterProvider>);
    fireEvent.click(await screen.findByRole("button", { name: /Fixture Compute Alpha/ }));
    expect(onInspect).toHaveBeenCalledTimes(1);
  });
});
