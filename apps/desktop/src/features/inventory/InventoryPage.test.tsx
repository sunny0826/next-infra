import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConnectionPurgeSummary } from "../../generated/query/ConnectionPurgeSummary";
import type { PageInfo } from "../../generated/query/PageInfo";
import type { RelationPageDto } from "../../generated/query/RelationPageDto";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import type { RelationsForResourcesInput } from "../../platform/desktop-adapter/desktop-adapter";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import {
  GitHubGoal5Adapter,
  createGitHubGoal5Adapter,
} from "../../test/fixtures/github-goal5-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { compactResourceId, InventoryPage } from "./InventoryPage";

afterEach(cleanup);

class PurgeTrackingAdapter extends MockDesktopAdapter {
  readonly purge = vi.fn();

  override async listConnections() {
    const page = await super.listConnections();
    return {
      ...page,
      items: page.items.map((connection) => ({
        ...connection,
        connector_type:
          connection.connection_id === "fixture-connection-alpha"
            ? "github"
            : connection.connection_id === "fixture-connection-beta"
              ? "ssh"
              : connection.connector_type,
      })),
    };
  }

  override async purgeConnection(connectionId: string): Promise<ConnectionPurgeSummary> {
    this.purge(connectionId);
    return this.previewConnectionPurge(connectionId);
  }
}

class PaginatedRelationsAdapter extends GitHubGoal5Adapter {
  readonly relationRequests = vi.fn();

  override async getRelationsForResources(
    input: RelationsForResourcesInput,
  ): Promise<RelationPageDto> {
    this.relationRequests(input);
    const page = await super.getRelationsForResources(input);
    if (input.cursor === undefined) {
      return {
        ...page,
        items: [],
        page_info: {
          next_cursor: "fixture-relations-next" as NonNullable<PageInfo["next_cursor"]>,
        },
      };
    }
    return page;
  }
}

class RepeatingRelationCursorAdapter extends GitHubGoal5Adapter {
  readonly relationRequests = vi.fn();

  override async getRelationsForResources(
    input: RelationsForResourcesInput,
  ): Promise<RelationPageDto> {
    this.relationRequests(input);
    const page = await super.getRelationsForResources(input);
    return {
      ...page,
      items: [],
      page_info: {
        next_cursor: "fixture-repeated-cursor" as NonNullable<PageInfo["next_cursor"]>,
      },
    };
  }
}

function renderPage(onSelectResource = vi.fn()) {
  render(
    <DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}>
      <InventoryPage onSelectResource={onSelectResource} />
    </DesktopAdapterProvider>,
  );
  return onSelectResource;
}

describe("InventoryPage", () => {
  it("keeps short resource ids and abbreviates long ids with stable endpoints", () => {
    expect(compactResourceId("fixture-resource-alpha")).toBe("fixture-resource-alpha");
    expect(compactResourceId("provider://account/region/resource/very-long-opaque-identifier", 28))
      .toBe("provider://acco…e-identifier");
  });

  it("renders health freshness lifecycle and observed time as separate columns", async () => {
    renderPage();
    expect(await screen.findByText("Fixture Compute Alpha")).toBeInTheDocument();
    for (const heading of ["健康度", "新鲜度", "生命周期", "观测时间"]) {
      expect(screen.getByRole("columnheader", { name: heading })).toBeInTheDocument();
    }
  });

  it("filters to attention resources without editing opaque cursors", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    fireEvent.click(screen.getByRole("button", { name: "需关注" }));
    expect(screen.queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
    expect(screen.getByText("Fixture Database Beta")).toBeInTheDocument();
  });

  it("filters to expired resources only", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    fireEvent.click(screen.getByRole("button", { name: "已过期" }));
    expect(screen.queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
    expect(screen.getByText("Fixture Database Beta")).toBeInTheDocument();
    expect(screen.getByText("Fixture Tombstoned Endpoint")).toBeInTheDocument();
    expect(screen.queryByText("Fixture Orphaned Worker")).not.toBeInTheDocument();
  });

  it("filters to removed resources only", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    fireEvent.click(screen.getByRole("button", { name: "已失效" }));
    expect(screen.queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
    expect(screen.queryByText("Fixture Database Beta")).not.toBeInTheDocument();
    expect(screen.getByText("Fixture Tombstoned Endpoint")).toBeInTheDocument();
    expect(screen.getByText("Fixture Orphaned Worker")).toBeInTheDocument();
  });

  it("shows the connection display name instead of the raw connection id", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    const alphaRow = screen.getByText("Fixture Compute Alpha").closest("tr")!;
    expect(within(alphaRow).getByText("Fixture Connection Alpha")).toBeInTheDocument();
    expect(within(alphaRow).queryByText("fixture-connection-alpha")).not.toBeInTheDocument();
  });

  it("shows the expiry panel and completes the purge preview to confirm flow", async () => {
    const adapter = new PurgeTrackingAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    render(
      <DesktopAdapterProvider adapter={adapter}>
        <InventoryPage />
      </DesktopAdapterProvider>,
    );

    await screen.findByText("Fixture Compute Alpha");

    const panel = screen.getByRole("region", { name: "过期数据清理" });
    expect(within(panel).getByText("Fixture Connection Alpha")).toBeInTheDocument();
    expect(within(panel).getByText("Fixture Connection Beta")).toBeInTheDocument();
    expect(within(panel).getByText("1 已过期 · 0 已失效")).toBeInTheDocument();
    expect(within(panel).getByText("1 已过期 · 2 已失效")).toBeInTheDocument();

    const purgeButtons = within(panel).getAllByRole("button", { name: "清理本地数据" });
    expect(purgeButtons).toHaveLength(2);
    fireEvent.click(purgeButtons[1]);

    const confirmation = await screen.findByRole("alert");
    expect(
      within(confirmation).getByRole("heading", { name: "删除本地快照" }),
    ).toBeInTheDocument();
    expect(within(confirmation).getByText("资源")).toBeInTheDocument();
    expect(within(confirmation).getByText("2")).toBeInTheDocument();

    fireEvent.click(within(confirmation).getByRole("button", { name: "确认删除本地快照" }));
    await waitFor(() => {
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });
    expect(adapter.purge).toHaveBeenCalledWith("fixture-connection-beta");
  });

  it("does not expose connection purge for unsupported connector types", async () => {
    renderPage();
    await screen.findByText("Fixture Compute Alpha");
    expect(screen.queryByRole("region", { name: "过期数据清理" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "清理本地数据" })).not.toBeInTheDocument();
  });

  it("selects a row with Enter", async () => {
    const onSelect = renderPage();
    const row = (await screen.findByText("Fixture Compute Alpha")).closest("tr");
    fireEvent.keyDown(row!, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders GitHub resources as a tree built from real relations", async () => {
    render(
      <DesktopAdapterProvider adapter={createGitHubGoal5Adapter()}>
        <InventoryPage />
      </DesktopAdapterProvider>,
    );

    expect(await screen.findByText("Fixture Repository")).toBeInTheDocument();
    expect(screen.getByText("Fixture Workflow")).toBeInTheDocument();
    expect(screen.getByText("Fixture Run")).toBeInTheDocument();

    const repositoryLine = screen
      .getByText("Fixture Repository")
      .closest("tr")!
      .querySelector(".inventory-tree-line") as HTMLElement;
    const workflowLine = screen
      .getByText("Fixture Workflow")
      .closest("tr")!
      .querySelector(".inventory-tree-line") as HTMLElement;
    const runLine = screen
      .getByText("Fixture Run")
      .closest("tr")!
      .querySelector(".inventory-tree-line") as HTMLElement;

    expect(repositoryLine.style.paddingLeft).toBe("0px");
    expect(workflowLine.style.paddingLeft).toBe("16px");
    // github.executes is operational, not containment — the run stays a root.
    expect(runLine.style.paddingLeft).toBe("0px");

    const repositoryRow = screen.getByText("Fixture Repository").closest("tr")!;
    expect(repositoryRow.querySelector(".inventory-disclosure")).not.toBeNull();
    const runRow = screen.getByText("Fixture Run").closest("tr")!;
    expect(runRow.querySelector(".inventory-disclosure")).toBeNull();

    fireEvent.click(repositoryRow.querySelector(".inventory-disclosure")!);
    expect(screen.queryByText("Fixture Workflow")).not.toBeInTheDocument();
    expect(screen.getByText("Fixture Run")).toBeInTheDocument();
    expect(screen.getByText("Fixture Repository")).toBeInTheDocument();
  });

  it("loads every relation page before building the tree", async () => {
    const adapter = new PaginatedRelationsAdapter();
    render(
      <DesktopAdapterProvider adapter={adapter}>
        <InventoryPage />
      </DesktopAdapterProvider>,
    );

    const workflowLine = (await screen.findByText("Fixture Workflow"))
      .closest("tr")!
      .querySelector(".inventory-tree-line") as HTMLElement;
    expect(workflowLine.style.paddingLeft).toBe("16px");
    expect(adapter.relationRequests).toHaveBeenCalledTimes(2);
    expect(adapter.relationRequests.mock.calls[1][0]).toMatchObject({
      cursor: "fixture-relations-next",
    });
  });

  it("stops repeated relation cursors and degrades to a flat list", async () => {
    const adapter = new RepeatingRelationCursorAdapter();
    render(
      <DesktopAdapterProvider adapter={adapter}>
        <InventoryPage />
      </DesktopAdapterProvider>,
    );

    const workflowLine = (await screen.findByText("Fixture Workflow"))
      .closest("tr")!
      .querySelector(".inventory-tree-line") as HTMLElement;
    expect(workflowLine.style.paddingLeft).toBe("0px");
    expect(adapter.relationRequests).toHaveBeenCalledTimes(2);
  });

  it("does not select a row when disclosure handles a keyboard event", async () => {
    const onSelect = vi.fn();
    render(
      <DesktopAdapterProvider adapter={createGitHubGoal5Adapter()}>
        <InventoryPage onSelectResource={onSelect} />
      </DesktopAdapterProvider>,
    );

    const repositoryRow = (await screen.findByText("Fixture Repository")).closest("tr")!;
    const disclosure = repositoryRow.querySelector(".inventory-disclosure")!;
    fireEvent.keyDown(disclosure, { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
  });
});
