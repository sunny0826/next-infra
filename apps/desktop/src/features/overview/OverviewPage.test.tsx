import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RouteId } from "../../app/routes";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { GitHubActionsSummarySnapshot } from "../../platform/desktop-adapter/desktop-adapter";
import {
  createGitHubGoal5SnapshotFixture,
  createQueryEvidenceLifecycleSnapshotFixture,
} from "../../test/fixtures/query-fixtures";
import { OverviewPage } from "./OverviewPage";

afterEach(cleanup);

interface RenderPageOptions {
  readonly adapter?: MockDesktopAdapter;
  readonly onNavigate?: (routeId: RouteId) => void;
}

function renderPage({
  adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture()),
  onNavigate,
}: RenderPageOptions = {}) {
  render(
    <DesktopAdapterProvider adapter={adapter}>
      <OverviewPage onNavigate={onNavigate} />
    </DesktopAdapterProvider>,
  );
}

class GitHubActionsSummaryAdapter extends MockDesktopAdapter {
  constructor(snapshot: ReturnType<typeof createGitHubGoal5SnapshotFixture>, private readonly summary: GitHubActionsSummarySnapshot) {
    super(snapshot);
  }
  override async getGitHubActionsSummary(): Promise<GitHubActionsSummarySnapshot> {
    return this.summary;
  }
}

class TruncatedResourcesAdapter extends MockDesktopAdapter {
  override async searchResources() {
    const page = await super.searchResources();
    return { ...page, items: page.items.slice(0, 2) };
  }
}

class AllHealthyResourcesAdapter extends MockDesktopAdapter {
  override async searchResources() {
    const page = await super.searchResources();
    return {
      ...page,
      items: page.items.map((item) => ({
        ...item,
        lifecycle: "active" as const,
        health: "healthy" as const,
        freshness: "fresh" as const,
      })),
    };
  }
}

describe("OverviewPage", () => {
  it("summarizes resources, connections, attention and the snapshot in one panel", async () => {
    renderPage();
    expect(await screen.findByText("共 4 个资源")).toBeInTheDocument();
    expect(screen.getByText("4 个资源")).toBeInTheDocument();
    expect(screen.getByText("3 个连接")).toBeInTheDocument();
    expect(screen.getByText("1 异常")).toBeInTheDocument();
    expect(screen.getByText("3 条事项")).toBeInTheDocument();
    expect(screen.getByText("上次快照")).toBeInTheDocument();
    expect(screen.getByText("总体可用，有 3 个事项需要你留意。")).toBeInTheDocument();
    expect(screen.queryByText(/仅基于前 25 个资源/)).not.toBeInTheDocument();
  });

  it("sorts attention rows by severity and shows plain-language reasons", async () => {
    renderPage();
    const firstRow = await screen.findByRole("button", { name: /Fixture Database Beta/ });
    const list = firstRow.closest(".overview-attention-list");
    if (list === null) throw new Error("attention list was not rendered");
    const rows = Array.from(list.querySelectorAll("button"));
    const names = rows.map((row) => row.textContent ?? "");
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
  });

  it("filters github.workflow_run from the attention list", async () => {
    renderPage({ adapter: new MockDesktopAdapter(createGitHubGoal5SnapshotFixture()) });
    expect(await screen.findByText("共 3 个资源")).toBeInTheDocument();
    expect(screen.queryByText(/Fixture Run/)).not.toBeInTheDocument();
  });

  it("navigates through the quick link tiles", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    renderPage({ onNavigate });
    await user.click(await screen.findByRole("button", { name: "资源清单 · 共 4 个资源" }));
    expect(onNavigate).toHaveBeenCalledWith("inventory");
    await user.click(screen.getByRole("button", { name: /连接器\s*·\s*3 个连接\s*·\s*1 异常/ }));
    expect(onNavigate).toHaveBeenCalledWith("connectors");
    await user.click(screen.getByRole("button", { name: "时间线 · 0 项变更" }));
    expect(onNavigate).toHaveBeenCalledWith("timeline");
    expect(onNavigate).toHaveBeenCalledTimes(3);
  });

  it("renders the GitHub Actions chip when summary data exists", async () => {
    const summary: GitHubActionsSummarySnapshot = {
      items: [{
        connection_id: "fixture-github-connection",
        connection_name: "GitHub Fixture Connection",
        repositories: [{
          repository_id: "fixture-github-repository-10",
          repository_name: "Fixture Repository",
          action_count: 4,
          succeeded: 3,
          failed: 1,
          running: 2,
        }],
      }],
    };
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    renderPage({
      adapter: new GitHubActionsSummaryAdapter(createGitHubGoal5SnapshotFixture(), summary),
      onNavigate,
    });
    const chip = await screen.findByRole("button", {
      name: "GitHub Actions · 通过率 75% · 2 运行中",
    });
    await user.click(chip);
    expect(onNavigate).toHaveBeenCalledWith("connectors");
  });

  it("omits the GitHub Actions chip when the summary is empty", async () => {
    renderPage();
    expect(await screen.findByText("共 4 个资源")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /GitHub Actions/ })).not.toBeInTheDocument();
  });

  it("notes when attention is computed from a truncated resource page", async () => {
    renderPage({
      adapter: new TruncatedResourcesAdapter(createQueryEvidenceLifecycleSnapshotFixture()),
    });
    expect(await screen.findByText("仅基于前 25 个资源计算。")).toBeInTheDocument();
  });

  it("shows an empty attention state with a link to the inventory", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    renderPage({
      adapter: new AllHealthyResourcesAdapter(createQueryEvidenceLifecycleSnapshotFixture()),
      onNavigate,
    });
    expect(await screen.findByText("没有需要关注的事项。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "查看全部资源" }));
    expect(onNavigate).toHaveBeenCalledWith("inventory");
  });
});
