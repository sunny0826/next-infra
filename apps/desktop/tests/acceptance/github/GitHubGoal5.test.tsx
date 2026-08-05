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
    expect(screen.getByText("degraded")).toBeInTheDocument();
    const repositories = screen.getByText("github.repositories").closest("article");
    expect(repositories).not.toBeNull();
    expect(within(repositories!).getByText("supported")).toBeInTheDocument();
    const deployments = screen.getByText("github.deployments").closest("article");
    expect(deployments).not.toBeNull();
    expect(within(deployments!).getByText("partial")).toBeInTheDocument();
    expect(within(deployments!).getByText(/status is not collected/i)).toBeInTheDocument();
  });

  it("filters the bounded inventory through the adapter without producing a fake empty state", async () => {
    const user = userEvent.setup();
    renderWithGitHubFixture(<InventoryPage />);

    expect(await screen.findByText("Fixture Repository")).toBeInTheDocument();
    await user.type(screen.getByRole("searchbox", { name: "Resource filter" }), "github.workflow_run");
    expect(await screen.findByText("Fixture Run")).toBeInTheDocument();
    expect(screen.queryByText("Fixture Repository")).not.toBeInTheDocument();
    expect(screen.getByText("1 visible resources")).toBeInTheDocument();
  });

  it("renders repository evidence paths to workflow and deployment with provider provenance", async () => {
    renderWithGitHubFixture(
      <ResourceDetailPage resourceId="fixture-github-repository-10" />,
    );

    expect(await screen.findByRole("heading", { name: "Fixture Repository" })).toBeInTheDocument();
    expect(screen.getByText("Fixture Workflow")).toBeInTheDocument();
    expect(screen.getAllByText("Fixture Environment").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Provider").length).toBeGreaterThan(0);
    expect(screen.getByText("visibility")).toBeInTheDocument();
    expect(screen.getByText("private")).toBeInTheDocument();
    expect(screen.getByText("11 declared modules")).toBeInTheDocument();
  });

  it("keeps workflow run and job health visible without inventing a critical path", async () => {
    renderWithGitHubFixture(<OverviewPage />);

    expect(await screen.findByText("6 bounded resources")).toBeInTheDocument();
    expect(screen.getByText("GitHub Fixture Connection")).toBeInTheDocument();
    expect(screen.getByText("No critical path is pinned. Next Infra will not infer importance from display names or recent activity.")).toBeInTheDocument();

    cleanup();
    renderWithGitHubFixture(
      <ResourceDetailPage resourceId="fixture-github-run-50" />,
    );
    expect(await screen.findByRole("heading", { name: "Fixture Run" })).toBeInTheDocument();
    expect(screen.getByText("Fixture Job")).toBeInTheDocument();
    expect(screen.getAllByText("healthy").length).toBeGreaterThan(0);
    expect(screen.getByText("run_attempt")).toBeInTheDocument();
  });
});
