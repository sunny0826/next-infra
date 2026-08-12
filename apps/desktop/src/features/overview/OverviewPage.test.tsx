import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RouteId } from "../../app/routes";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import type { GetResourceInput } from "../../platform/desktop-adapter/desktop-adapter";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { DesktopAdapterSnapshot } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import {
  createGitHubGoal5SnapshotFixture,
  createQueryEvidenceLifecycleSnapshotFixture,
} from "../../test/fixtures/query-fixtures";
import { OverviewPage } from "./OverviewPage";
import { OverviewEvidenceAdapter } from "./overview-evidence-adapter";

afterEach(cleanup);

interface RenderPageOptions {
  readonly adapter?: MockDesktopAdapter;
  readonly onNavigate?: (routeId: RouteId) => void;
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
}

function renderPage({
  adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture()),
  onNavigate,
  onInspectResource,
  onInspectRelation,
}: RenderPageOptions = {}) {
  render(
    <DesktopAdapterProvider adapter={adapter}>
      <OverviewPage
        onNavigate={onNavigate}
        onInspectResource={onInspectResource}
        onInspectRelation={onInspectRelation}
      />
    </DesktopAdapterProvider>,
  );
}

function adapterFor(
  mutate: (snapshot: DesktopAdapterSnapshot) => DesktopAdapterSnapshot,
): MockDesktopAdapter {
  return new MockDesktopAdapter(mutate(createQueryEvidenceLifecycleSnapshotFixture()));
}

function healthyResources(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    ...snapshot,
    resources: snapshot.resources.map((resource) => ({
      ...resource,
      lifecycle: "active" as const,
      health: "healthy" as const,
      freshness: "fresh" as const,
    })),
  };
}

function healthyConnections(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    ...snapshot,
    connections: snapshot.connections.map((connection) => ({
      ...connection,
      health: "healthy" as const,
    })),
  };
}

/** Expired and stale resources with healthy connections; no unhealthy/degraded resources. */
function staleFreshness(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    ...snapshot,
    resources: snapshot.resources.map((resource, index) => ({
      ...resource,
      lifecycle: "active" as const,
      health: "healthy" as const,
      freshness: (index === 1 ? "expired" : index > 1 ? "stale" : "fresh") as ResourceDto["freshness"],
    })),
    connections: snapshot.connections.map((connection) => ({
      ...connection,
      health: "healthy" as const,
    })),
  };
}

/** The default snapshot with one resource flipped to unhealthy, everything else unchanged. */
function unhealthyResource(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    ...snapshot,
    resources: snapshot.resources.map((resource) =>
      resource.resource_id === "fixture-resource-alpha"
        ? {
            ...resource,
            lifecycle: "active" as const,
            health: "unhealthy" as const,
            freshness: "fresh" as const,
          }
        : resource,
    ),
  };
}

class TruncatedResourcesAdapter extends MockDesktopAdapter {
  override async searchResources() {
    const page = await super.searchResources();
    return { ...page, items: page.items.slice(0, 2) };
  }
}

class LazyReadSpyAdapter extends OverviewEvidenceAdapter {
  reads: string[] = [];
  override async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    this.reads = [...this.reads, input.resource_id];
    return super.getResource(input);
  }
}

/** Relation reads never settle, so the panel stays in its loading state. */
class PendingEvidenceAdapter extends OverviewEvidenceAdapter {
  override async getResource(_input: GetResourceInput): Promise<ResourceDetailDto> {
    return new Promise<ResourceDetailDto>(() => {});
  }
}

/** Relation reads fail, so the panel reports an error instead of a summary. */
class FailingEvidenceAdapter extends OverviewEvidenceAdapter {
  override async getResource(_input: GetResourceInput): Promise<ResourceDetailDto> {
    throw new Error("fixture relation read failed");
  }
}

/**
 * Every relation read waits for resolveNext(); enqueued responses win, and
 * unqueued calls fall back to the fixture detail. Lets a test drive the
 * loading→ready transition without timers.
 */
class DeferredEvidenceAdapter extends OverviewEvidenceAdapter {
  readonly #pending: Array<() => void> = [];
  readonly #responses: ResourceDetailDto[] = [];
  override async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    await new Promise<void>((resolve) => this.#pending.push(resolve));
    const detail = this.#responses.shift();
    if (detail !== undefined) return detail;
    return super.getResource(input);
  }
  enqueue(detail: ResourceDetailDto): void {
    this.#responses.push(detail);
  }
  resolveNext(): void {
    this.#pending.shift()?.();
  }
}

function betaDetailWith(relations: readonly RelationDto[]): ResourceDetailDto {
  const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
  const metadata = snapshot.metadata;
  const beta = snapshot.resources.find(
    (resource) => resource.resource_id === "fixture-resource-beta",
  );
  if (metadata === null || beta === undefined) {
    throw new Error("beta fixture resource or metadata is missing");
  }
  return {
    metadata,
    resource: beta,
    attributes: {},
    relations: [...relations],
    recent_changes: [],
    connector_coverage: [],
  };
}

/** The two fixed row actions; every other button inside an item is the relation summary. */
const ROW_ACTIONS = new Set(["查看资源", "核验证据", "收起证据"]);

function relationSummaryButton(item: HTMLElement): HTMLElement {
  const candidates = within(item)
    .getAllByRole("button")
    .filter((button) => !ROW_ACTIONS.has(button.textContent?.trim() ?? ""));
  expect(candidates).toHaveLength(1);
  return candidates[0];
}

function attentionItem(viewButton: HTMLElement): HTMLElement {
  const item = viewButton.closest(".overview-attention-item");
  if (item === null) throw new Error("attention item wrapper was not rendered");
  return item as HTMLElement;
}

describe("OverviewPage", () => {
  it("shows a state conclusion with 待核验/异常连接/快照 secondary info and no KPI totals", async () => {
    renderPage();
    expect(await screen.findByText("有 1 个资源处于降级状态。")).toBeInTheDocument();

    const pending = screen.getByText("待核验").closest(".overview-summary-cell");
    expect(pending).not.toBeNull();
    expect(pending).toHaveTextContent("3 项");

    const connections = screen.getByText("异常连接").closest(".overview-summary-cell");
    expect(connections).not.toBeNull();
    expect(connections).toHaveTextContent("1");

    const snapshot = screen.getByText("快照").closest(".overview-summary-cell");
    expect(snapshot).not.toBeNull();
    expect(snapshot).toHaveTextContent("2000-01-01");

    // The frozen contract drops resource/connection totals, the GitHub chip,
    // and change counts from the summary.
    expect(screen.queryByText("共 4 个资源")).not.toBeInTheDocument();
    expect(screen.queryByText("4 个资源")).not.toBeInTheDocument();
    expect(screen.queryByText("3 个连接")).not.toBeInTheDocument();
    expect(screen.queryByText("1 异常")).not.toBeInTheDocument();
    expect(screen.queryByText(/GitHub Actions/)).not.toBeInTheDocument();
    expect(screen.queryByText(/项变更/)).not.toBeInTheDocument();
  });

  it("prioritizes the conclusion: unhealthy before degraded, freshness, and connectors", async () => {
    renderPage({ adapter: adapterFor(unhealthyResource) });
    expect(
      await screen.findByText("有 1 个资源异常，需要优先处理。"),
    ).toBeInTheDocument();
  });

  it("prioritizes degraded resources over stale data and abnormal connectors", async () => {
    renderPage();
    expect(await screen.findByText("有 1 个资源处于降级状态。")).toBeInTheDocument();
  });

  it("concludes expired or stale observation data when resources are healthy", async () => {
    renderPage({ adapter: adapterFor(staleFreshness) });
    expect(
      await screen.findByText("有 3 个资源的观察数据过期或较旧。"),
    ).toBeInTheDocument();
  });

  it("concludes an abnormal observation chain when only connectors are unhealthy", async () => {
    renderPage({ adapter: adapterFor(healthyResources) });
    expect(
      await screen.findByText("有 1 个连接异常，观测链路不完整。"),
    ).toBeInTheDocument();
  });

  it("concludes no pending items when resources and connectors are all healthy", async () => {
    renderPage({ adapter: adapterFor((snapshot) => healthyResources(healthyConnections(snapshot))) });
    expect(await screen.findByText("总体健康，没有待处理事项。")).toBeInTheDocument();
  });

  it("sorts attention rows by severity and shows plain-language reasons", async () => {
    renderPage();
    const firstRow = (await screen.findAllByRole("button", { name: "查看资源" }))[0];
    const list = firstRow.closest(".overview-attention-list");
    if (list === null) throw new Error("attention list was not rendered");
    const items = Array.from(list.querySelectorAll(".overview-attention-item"));
    const names = items.map((item) => item.textContent ?? "");
    expect(names).toHaveLength(3);
    const expectedOrder = [
      "Fixture Database Beta",
      "Fixture Tombstoned Endpoint",
      "Fixture Orphaned Worker",
    ];
    expect(
      expectedOrder.map((name) => names.findIndex((text) => text.includes(name))),
    ).toEqual([0, 1, 2]);
    expect(screen.getAllByText("最后更新")).toHaveLength(2);
    expect(screen.getByText("状态降级")).toBeInTheDocument();
    expect(screen.getAllByText("已过期")).toHaveLength(2);
    expect(screen.getByText("降级")).toBeInTheDocument();
    const times = document.querySelectorAll('time[dateTime="2000-01-01T00:00:00Z"]');
    expect(times).toHaveLength(3);
    for (const time of times) {
      expect(time).toHaveTextContent("2000-01-01");
      expect(time).toHaveAttribute("title", "2000-01-01T00:00:00Z");
    }
    // Every item exposes the explicit 查看资源 and 核验证据 actions.
    for (const item of items) {
      expect(within(item as HTMLElement).getByRole("button", { name: "查看资源" })).toBeInTheDocument();
      expect(within(item as HTMLElement).getByRole("button", { name: "核验证据" })).toBeInTheDocument();
    }
  });

  it("filters github.workflow_run from the attention list", async () => {
    renderPage({ adapter: new MockDesktopAdapter(createGitHubGoal5SnapshotFixture()) });
    expect(await screen.findByText("没有需要关注的事项。")).toBeInTheDocument();
    expect(screen.queryByText(/Fixture Run/)).not.toBeInTheDocument();
    expect(screen.getByText("有 1 个连接异常，观测链路不完整。")).toBeInTheDocument();
  });

  it("inspects a resource through the explicit 查看资源 action", async () => {
    const user = userEvent.setup();
    const onInspectResource = vi.fn();
    renderPage({ onInspectResource });
    const view = (await screen.findAllByRole("button", { name: "查看资源" }))[0];
    await user.click(view);
    expect(onInspectResource).toHaveBeenCalledTimes(1);
    expect(onInspectResource).toHaveBeenCalledWith(
      expect.objectContaining({ resource_id: "fixture-resource-beta" }),
    );
  });

  it("navigates through the three quiet footer entries without duplicate counts", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    renderPage({ onNavigate });
    await user.click(await screen.findByRole("button", { name: "查看全部资源" }));
    expect(onNavigate).toHaveBeenCalledWith("inventory");
    await user.click(screen.getByRole("button", { name: "查看异常连接" }));
    expect(onNavigate).toHaveBeenCalledWith("connectors");
    await user.click(screen.getByRole("button", { name: "查看最近变更" }));
    expect(onNavigate).toHaveBeenCalledWith("timeline");
    expect(onNavigate).toHaveBeenCalledTimes(3);
  });

  it("qualifies the pending count as items within the first 25 resources when truncated", async () => {
    renderPage({
      adapter: new TruncatedResourcesAdapter(createQueryEvidenceLifecycleSnapshotFixture()),
    });
    expect(await screen.findByText("前 25 个资源中 1 项")).toBeInTheDocument();
    expect(screen.getByText(/资源页被截断/)).toBeInTheDocument();
    expect(screen.getByText(/不代表全局总数/)).toBeInTheDocument();
    // The truncated count is scoped to the page, never presented as a global total.
    expect(screen.queryByText(/共 \d+ 个资源/)).not.toBeInTheDocument();
  });

  it("shows an empty attention state with a link to the inventory", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    renderPage({
      adapter: adapterFor(healthyResources),
      onNavigate,
    });
    const empty = await screen.findByText("没有需要关注的事项。");
    const emptyState = empty.closest(".overview-empty");
    if (emptyState === null) throw new Error("empty attention state was not rendered");
    await user.click(within(emptyState as HTMLElement).getByRole("button", { name: "查看全部资源" }));
    expect(onNavigate).toHaveBeenCalledWith("inventory");
  });

  it("reads evidence lazily only after the disclosure is opened", async () => {
    const user = userEvent.setup();
    const adapter = new LazyReadSpyAdapter();
    renderPage({ adapter });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    expect(adapter.reads).toHaveLength(0);
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));
    expect(adapter.reads).toContain("fixture-resource-beta");
    expect(await within(firstItem).findByText("Fixture Compute Alpha")).toBeInTheDocument();
  });

  it("shows one compact relation summary per group instead of an inline evidence spine", async () => {
    const user = userEvent.setup();
    const onInspectRelation = vi.fn();
    renderPage({ adapter: new OverviewEvidenceAdapter(), onInspectRelation });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));

    // The compact summary names both endpoints, counts the group's relations,
    // and humanizes the relation kind instead of leaking the raw value.
    expect(await within(firstItem).findByText("Fixture Compute Alpha")).toBeInTheDocument();
    // The target name appears twice: the row identity and the compact summary.
    expect(within(firstItem).getAllByText("Fixture Database Beta").length).toBeGreaterThanOrEqual(2);
    const summary = relationSummaryButton(firstItem);
    expect(summary.textContent).toMatch(/3\s*条关系/);
    expect(summary.textContent).toContain("依赖");
    expect(summary.textContent).not.toContain("fixture.depends_on");

    // No inline full spine: no evidence cards, facts, or chain markup.
    expect(within(firstItem).queryByText("证据链")).not.toBeInTheDocument();
    expect(within(firstItem).queryByText("当前事实")).not.toBeInTheDocument();
    expect(within(firstItem).queryByText("提供方")).not.toBeInTheDocument();
    expect(within(firstItem).queryByText("已配置")).not.toBeInTheDocument();
    expect(within(firstItem).queryByText("推断")).not.toBeInTheDocument();
    expect(firstItem.querySelectorAll(".evidence-spine__step")).toHaveLength(0);

    // Activating the summary delegates to the existing Inspector via onInspectRelation.
    expect(summary).not.toBeDisabled();
    await user.click(summary);
    expect(onInspectRelation).toHaveBeenCalledTimes(1);
    const relation = onInspectRelation.mock.calls[0][0] as RelationDto;
    expect(relation).toMatchObject({
      source_resource_id: "fixture-resource-alpha",
      target_resource_id: "fixture-resource-beta",
    });
    expect(relation.relation_id).toMatch(
      /^fixture-relation-(provider|configured|inferred)-alpha-beta$/,
    );
  });

  it("disables the relation summary when no inspector callback is provided", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new OverviewEvidenceAdapter() });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));
    await within(firstItem).findByText("Fixture Compute Alpha");

    const summary = relationSummaryButton(firstItem);
    expect(summary).toBeDisabled();
    expect(summary.getAttribute("title")).toMatch(/检查器不可用/);
  });

  it("points the disclosure aria-controls at the mounted panel and unmounts it when collapsed", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new OverviewEvidenceAdapter() });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));

    const toggle = within(firstItem).getByRole("button", { name: "收起证据" });
    const controlsId = toggle.getAttribute("aria-controls");
    expect(controlsId).not.toBeNull();
    expect(controlsId).not.toBe("");
    expect(await within(firstItem).findByText("Fixture Compute Alpha")).toBeInTheDocument();
    expect(document.getElementById(controlsId as string)).not.toBeNull();

    await user.click(toggle);
    expect(document.getElementById(controlsId as string)).toBeNull();
    expect(within(firstItem).getByRole("button", { name: "核验证据" })).toBeInTheDocument();
  });

  it("returns to loading on queryVersion refresh instead of keeping the stale summary", async () => {
    const user = userEvent.setup();
    const adapter = new DeferredEvidenceAdapter();
    const view = render(
      <DesktopAdapterProvider adapter={adapter}>
        <OverviewPage queryVersion={0} />
      </DesktopAdapterProvider>,
    );
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );

    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));
    expect(await within(firstItem).findByText(/正在读取证据/)).toBeInTheDocument();
    adapter.resolveNext();
    await within(firstItem).findByText("Fixture Compute Alpha");
    const summary = relationSummaryButton(firstItem);
    expect(summary.textContent).toMatch(/3\s*条关系/);

    view.rerender(
      <DesktopAdapterProvider adapter={adapter}>
        <OverviewPage queryVersion={1} />
      </DesktopAdapterProvider>,
    );
    // The overview reloads; the panel re-reads and drops the stale summary.
    expect(await within(firstItem).findByText(/正在读取证据/)).toBeInTheDocument();
    expect(within(firstItem).queryByText(/条关系/)).not.toBeInTheDocument();

    adapter.enqueue(
      betaDetailWith([createQueryEvidenceLifecycleSnapshotFixture().relations[0]]),
    );
    adapter.resolveNext();
    expect(await within(firstItem).findByText("1 条关系 · 依赖")).toBeInTheDocument();
  });

  it("keeps the panel loading while the relation read is pending", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new PendingEvidenceAdapter() });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));
    expect(await within(firstItem).findByText(/正在读取证据/)).toBeInTheDocument();
  });

  it("reports an error when the relation read fails", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new FailingEvidenceAdapter() });
    const firstItem = attentionItem(
      (await screen.findAllByRole("button", { name: "查看资源" }))[0],
    );
    await user.click(within(firstItem).getByRole("button", { name: "核验证据" }));
    expect(await within(firstItem).findByText(/无法读取证据/)).toBeInTheDocument();
  });

  it("shows the evidence empty state for an attention item without relations", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new OverviewEvidenceAdapter() });
    const viewButtons = await screen.findAllByRole("button", { name: "查看资源" });
    const tombstonedItem = attentionItem(viewButtons[1]);
    await user.click(within(tombstonedItem).getByRole("button", { name: "核验证据" }));
    expect(
      await within(tombstonedItem).findByText(/未发现关联证据|没有可用证据/),
    ).toBeInTheDocument();
  });

  it("keeps only one evidence area expanded at a time", async () => {
    const user = userEvent.setup();
    renderPage({ adapter: new OverviewEvidenceAdapter() });
    const viewButtons = await screen.findAllByRole("button", { name: "查看资源" });
    const items = viewButtons.map(attentionItem);
    await user.click(within(items[0]).getByRole("button", { name: "核验证据" }));
    expect(await within(items[0]).findByText("Fixture Compute Alpha")).toBeInTheDocument();

    await user.click(within(items[1]).getByRole("button", { name: "核验证据" }));
    expect(
      await within(items[1]).findByText(/未发现关联证据|没有可用证据/),
    ).toBeInTheDocument();
    expect(within(items[0]).queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
    expect(within(items[0]).getByRole("button", { name: "核验证据" })).toBeInTheDocument();
    expect(within(items[1]).getByRole("button", { name: "收起证据" })).toBeInTheDocument();
  });
});
