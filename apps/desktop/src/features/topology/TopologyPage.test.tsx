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
    expect(await screen.findByText(/上游来源/)).toBeInTheDocument();
    expect(screen.getByText(/当前焦点/)).toBeInTheDocument();
    expect(screen.getByText(/下游目标/)).toBeInTheDocument();
    expect(await screen.findByText("提供方 · 实线")).toBeInTheDocument();
    expect(screen.getByText("已配置 · 双线")).toBeInTheDocument();
    expect(screen.getByText("推断 · 虚线")).toBeInTheDocument();
    expect(screen.getByText("fixture.depends_on · 人工声明")).toBeInTheDocument();
  });

  it("selects a node for the inspector", async () => {
    const onInspect = vi.fn();
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" onInspectResource={onInspect} /></DesktopAdapterProvider>);
    fireEvent.click(await screen.findByRole("button", { name: /Fixture Compute Alpha/ }));
    expect(onInspect).toHaveBeenCalledTimes(1);
  });

  it("moves arrow-key focus only across the bounded adjacency", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" /></DesktopAdapterProvider>);
    const alpha = await screen.findByRole("button", { name: /Fixture Compute Alpha/ });
    alpha.focus();
    fireEvent.keyDown(alpha, { key: "ArrowRight" });
    expect(screen.getByRole("button", { name: /Fixture Database Beta/ })).toHaveFocus();
  });

  it("routes toolbar create to the focused resource without an inline mutation form", async () => {
    const onCreateRelation = vi.fn();
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" onCreateRelation={onCreateRelation} /></DesktopAdapterProvider>);

    fireEvent.click(await screen.findByRole("button", { name: "新增关联" }));
    expect(onCreateRelation).toHaveBeenCalledWith(expect.objectContaining({ resource_id: "fixture-resource-alpha" }));
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByText("选择受限资源")).not.toBeInTheDocument();
  });

  it("offers edit only for configured evidence and keeps every edge inspectable", async () => {
    const onEditRelation = vi.fn();
    const onInspectRelation = vi.fn();
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><TopologyPage focusResourceId="fixture-resource-alpha" onEditRelation={onEditRelation} onInspectRelation={onInspectRelation} /></DesktopAdapterProvider>);

    const configured = await screen.findByRole("button", { name: "已配置关系 fixture.depends_on" });
    fireEvent.click(configured);
    const edit = screen.getByRole("button", { name: "编辑关联 fixture.depends_on" });
    fireEvent.click(edit);
    expect(onEditRelation).toHaveBeenCalledWith(expect.objectContaining({ evidence_type: "configured" }));
    expect(onInspectRelation).toHaveBeenCalledWith(expect.objectContaining({ evidence_type: "configured" }));

    fireEvent.click(screen.getByRole("button", { name: "提供方关系 fixture.depends_on" }));
    expect(onInspectRelation).toHaveBeenCalledWith(expect.objectContaining({ evidence_type: "provider" }));
    expect(screen.queryByRole("button", { name: "编辑关联 fixture.depends_on" })).not.toBeInTheDocument();
  });

  it("keeps an orphaned configured edge with an explicit unresolved placeholder", async () => {
    const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
    const adapter = new MockDesktopAdapter({
      ...snapshot,
      resources: snapshot.resources.filter((resource) => resource.resource_id !== "fixture-resource-orphaned"),
      relations: snapshot.relations.map((relation) => relation.relation_id === "fixture-relation-configured-alpha-beta"
        ? { ...relation, lifecycle: "orphaned" as const, target_resource_id: "fixture-resource-missing" }
        : relation),
    });
    const onInspectResource = vi.fn();
    const onEditRelation = vi.fn();
    render(<DesktopAdapterProvider adapter={adapter}><TopologyPage focusResourceId="fixture-resource-alpha" onEditRelation={onEditRelation} onInspectResource={onInspectResource} /></DesktopAdapterProvider>);

    const placeholder = await screen.findByLabelText("未解析资源 fixture-resource-missing");
    expect(placeholder.tagName).toBe("DIV");
    expect(screen.getByText("fixture.depends_on · 人工声明 · 未解析")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已配置关系 fixture.depends_on" }));
    expect(screen.getByRole("button", { name: "编辑关联 fixture.depends_on" })).toBeInTheDocument();
    fireEvent.click(placeholder);
    expect(onInspectResource).not.toHaveBeenCalled();
  });

  it("omits tombstoned configured edges from the active topology", async () => {
    const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
    const adapter = new MockDesktopAdapter({
      ...snapshot,
      relations: snapshot.relations.map((relation) => relation.relation_id === "fixture-relation-configured-alpha-beta"
        ? { ...relation, lifecycle: "tombstoned" as const }
        : relation),
    });
    render(<DesktopAdapterProvider adapter={adapter}><TopologyPage focusResourceId="fixture-resource-alpha" /></DesktopAdapterProvider>);

    expect(await screen.findByText("提供方 · 实线")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "已配置关系 fixture.depends_on" })).not.toBeInTheDocument();
    expect(screen.queryByText("fixture.depends_on · 人工声明")).not.toBeInTheDocument();
  });
});
