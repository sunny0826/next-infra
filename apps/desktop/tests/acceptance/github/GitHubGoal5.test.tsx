import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { ConnectorsPage } from "../../../src/features/connectors/ConnectorsPage";
import { InventoryPage } from "../../../src/features/inventory/InventoryPage";
import { OverviewPage } from "../../../src/features/overview/OverviewPage";
import { ResourceDetailPage } from "../../../src/features/resource-detail/ResourceDetailPage";
import { DesktopAdapterProvider } from "../../../src/platform/desktop-adapter/DesktopAdapterContext";
import { createGitHubGoal5Adapter } from "../../../src/test/fixtures/github-goal5-adapter";

afterEach(cleanup);

function renderWithGitHubFixture(node: ReactNode) {
  render(
    <DesktopAdapterProvider adapter={createGitHubGoal5Adapter()}>
      {node}
    </DesktopAdapterProvider>,
  );
}

describe("UI-G5-01 GitHub vertical acceptance", () => {
  it("shows connector health separately from supported and partial module coverage", async () => {
    renderWithGitHubFixture(<ConnectorsPage />);

    expect(await screen.findByText("GitHub Fixture Connection")).toBeInTheDocument();
    expect(screen.getByText("降级")).toBeInTheDocument();
    const repositories = screen.getByText("github.repositories").closest("article");
    expect(repositories).not.toBeNull();
    expect(within(repositories!).getByText("支持")).toBeInTheDocument();
    const actionsWorkflows = screen.getByText("github.actions.workflows").closest("article");
    expect(actionsWorkflows).not.toBeNull();
    expect(within(actionsWorkflows!).getByText("支持")).toBeInTheDocument();
  });

  it("filters the bounded inventory through the adapter without producing a fake empty state", async () => {
    const user = userEvent.setup();
    renderWithGitHubFixture(<InventoryPage />);

    expect(await screen.findByText("Fixture Repository")).toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "资源筛选" }), "github.workflow_run");
    expect(await screen.findByText("Fixture Run")).toBeInTheDocument();
    expect(screen.queryByText("Fixture Repository")).not.toBeInTheDocument();
    expect(screen.getByText("1 个可见资源")).toBeInTheDocument();
  });

  it("renders repository evidence paths to workflow with provider provenance", async () => {
    renderWithGitHubFixture(
      <ResourceDetailPage resourceId="fixture-github-repository-10" />,
    );

    expect(await screen.findByRole("heading", { name: "Fixture Repository" })).toBeInTheDocument();
    expect(screen.getByText("Fixture Workflow")).toBeInTheDocument();
    expect(screen.getAllByText("提供方").length).toBeGreaterThan(0);
    expect(screen.getByText("visibility")).toBeInTheDocument();
    expect(screen.getByText("private")).toBeInTheDocument();
    expect(screen.getByText("5 个声明模块")).toBeInTheDocument();
  });

  it("keeps workflow run health visible without inventing a critical path", async () => {
    renderWithGitHubFixture(<OverviewPage />);

    expect(await screen.findByText("共 3 个资源")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /连接器\s*·\s*1 个连接\s*·\s*1 异常/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("总体可用，有 1 个连接异常需要你留意。"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/关键路径/)).not.toBeInTheDocument();

    cleanup();
    renderWithGitHubFixture(
      <ResourceDetailPage resourceId="fixture-github-run-50" />,
    );
    expect(await screen.findByRole("heading", { name: "Fixture Run" })).toBeInTheDocument();
    expect(screen.getAllByText("健康").length).toBeGreaterThan(0);
    expect(screen.getByText("run_id")).toBeInTheDocument();
  });
});
