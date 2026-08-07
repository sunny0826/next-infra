import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createConnectorCoverageFixtures, createGitHubGoal5SnapshotFixture, createGoal9ConnectorCoverageFixtures, createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { ConnectorsPage } from "./ConnectorsPage";

afterEach(cleanup);

class ConnectorAdapter extends MockDesktopAdapter {
  override async listConnectorCoverage() { return { metadata: (await this.searchResources()).metadata, items: [...createConnectorCoverageFixtures(), ...createGoal9ConnectorCoverageFixtures()] }; }
}

describe("ConnectorsPage", () => {
  it("separates connector health and declared coverage", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("不可达")).toBeInTheDocument();
    expect(screen.getByText("fixture.compute")).toBeInTheDocument();
    expect(screen.getAllByText("支持").length).toBeGreaterThan(0);
  });

  it("limits manual sync to GitHub connections", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect((await screen.findAllByRole("button", { name: "手动同步" }))[0]).toBeDisabled();
  });

  it("clears the GitHub token field after a connection request", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    const name = await screen.findByLabelText("连接名称");
    const token = screen.getByLabelText("细粒度 Token");
    fireEvent.change(name, { target: { value: "Personal GitHub" } });
    fireEvent.change(token, { target: { value: "test-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并加载仓库" }));
    fireEvent.click(await screen.findByLabelText("fixture/first-repository"));
    fireEvent.click(screen.getByRole("button", { name: "创建连接并同步 1 个仓库" }));

    expect(await screen.findByText(/GitHub 连接已创建，将在后台同步 1 个选定仓库/)).toBeInTheDocument();
    expect(token).toHaveValue("");
  });

  it("renders Goal 9 modules as separate coverage rows", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("连接器覆盖矩阵")).toBeInTheDocument();
    expect(screen.getByText("supabase.managed.projects")).toBeInTheDocument();
    expect(screen.getByText("supabase.self_hosted.service_api")).toBeInTheDocument();
    expect(screen.getByText("aliyun.compute.ecs")).toBeInTheDocument();
    expect(screen.getByText("tencent.edge.clb")).toBeInTheDocument();
  });

  it("requires a separate confirmation before deleting one GitHub connection snapshot", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createGitHubGoal5SnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);

    fireEvent.click(await screen.findByRole("button", { name: "删除本地数据" }));

    expect(await screen.findByRole("heading", { name: "删除本地快照" })).toBeInTheDocument();
    expect(screen.getByText(/将永久删除“GitHub Fixture Connection”/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认删除本地快照" }));
    expect(await screen.findByText(/已删除 3 个资源、2 条关系/)).toBeInTheDocument();
  });
});
