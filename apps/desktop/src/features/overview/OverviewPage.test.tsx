import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { GitHubActionsSummarySnapshot } from "../../platform/desktop-adapter/desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture, createGitHubGoal5SnapshotFixture } from "../../test/fixtures/query-fixtures";
import { OverviewPage } from "./OverviewPage";

afterEach(cleanup);

function renderPage(adapter: MockDesktopAdapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())) {
  render(
    <DesktopAdapterProvider adapter={adapter}>
      <OverviewPage />
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

  it("shows empty GitHub Actions state when no summary data", async () => {
    renderPage();
    expect(
      await screen.findByText(/没有已同步的 GitHub Actions 数据/),
    ).toBeInTheDocument();
  });

  it("filters github.workflow_run from attention queue", async () => {
    const githubAdapter = new MockDesktopAdapter(createGitHubGoal5SnapshotFixture());
    renderPage(githubAdapter);
    expect(screen.queryByText(/Fixture Run/)).not.toBeInTheDocument();
  });

  it("renders GitHub Actions aggregation when summary is populated", async () => {
    const summary: GitHubActionsSummarySnapshot = {
      items: [{
        connection_id: "fixture-github-connection",
        connection_name: "GitHub Fixture Connection",
        repositories: [{
          repository_id: "fixture-github-repository-10",
          repository_name: "Fixture Repository",
          action_count: 1,
          succeeded: 1,
          failed: 0,
          running: 0,
        }],
      }],
    };
    const githubAdapter = new GitHubActionsSummaryAdapter(createGitHubGoal5SnapshotFixture(), summary);
    renderPage(githubAdapter);
    const h3Elements = await screen.findAllByText("GitHub Fixture Connection");
    const h3InGithubSection = h3Elements.find((el) => el.tagName === "H3");
    expect(h3InGithubSection).toBeInTheDocument();
    expect(screen.getByText("Fixture Repository")).toBeInTheDocument();
  });
});
