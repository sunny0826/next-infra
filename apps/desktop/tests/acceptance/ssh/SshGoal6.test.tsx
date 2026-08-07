import { cleanup, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { ConnectorsPage } from "../../../src/features/connectors/ConnectorsPage";
import { InventoryPage } from "../../../src/features/inventory/InventoryPage";
import { ResourceDetailPage } from "../../../src/features/resource-detail/ResourceDetailPage";
import { DesktopAdapterProvider } from "../../../src/platform/desktop-adapter/DesktopAdapterContext";
import { createSshGoal6Adapter } from "../../../src/test/fixtures/ssh-goal6-adapter";

afterEach(cleanup);

function renderWithSshFixture(node: ReactNode) {
  render(
    <DesktopAdapterProvider adapter={createSshGoal6Adapter()}>
      {node}
    </DesktopAdapterProvider>,
  );
}

describe("UI-G6-01 SSH vertical acceptance", () => {
  it("separates unreachable connector health from last-known resource health", async () => {
    renderWithSshFixture(<ConnectorsPage />);
    const connection = await screen.findByText("Fixture SSH Connection Alpha");
    const row = connection.closest("tr");
    expect(row).not.toBeNull();
    expect(within(row!).getByText("不可达")).toBeInTheDocument();
    expect(within(row!).getByText(/network_unreachable/)).toBeInTheDocument();

    cleanup();
    renderWithSshFixture(<InventoryPage />);
    const host = await screen.findByText("Fixture SSH Host Alpha");
    const hostRow = host.closest("tr");
    expect(hostRow).not.toBeNull();
    expect(within(hostRow!).getByText("健康")).toBeInTheDocument();
    expect(within(hostRow!).getByText("已过时")).toBeInTheDocument();
    expect(within(hostRow!).queryByText("不健康")).not.toBeInTheDocument();
  });

  it("shows the sanitized host key failure without a trust action", async () => {
    renderWithSshFixture(<ConnectorsPage />);
    const connection = await screen.findByText("Fixture SSH Connection Beta");
    const row = connection.closest("tr");
    expect(row).not.toBeNull();
    expect(within(row!).getByText(/host_key_mismatch/)).toBeInTheDocument();
    expect(within(row!).getByText(/trust was not changed/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /accept|trust/i })).not.toBeInTheDocument();
  });

  it("keeps provider evidence and sanitized host attributes inspectable", async () => {
    renderWithSshFixture(
      <ResourceDetailPage resourceId="fixture-ssh-host-unreachable" />,
    );
    expect(
      await screen.findByRole("heading", { name: "Fixture SSH Host Alpha" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Fixture SSH Filesystems Alpha")).toBeInTheDocument();
    expect(screen.getAllByText("提供方").length).toBeGreaterThan(0);
    expect(screen.getByText("uptime_bucket")).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/ssh-rsa|192\.168\.|10\.0\.|bearer/i);
  });
});
