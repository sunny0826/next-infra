import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { SshValidateResult, DokployValidateResult } from "../../platform/desktop-adapter/desktop-adapter";
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

  it("renders the SSH connection form section", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByRole("heading", { name: "添加 SSH 连接" })).toBeInTheDocument();
    expect(screen.getByLabelText("SSH 连接名称")).toBeInTheDocument();
    expect(screen.getByLabelText(/主机别名/)).toBeInTheDocument();
    expect(screen.getByLabelText("连接超时（秒）")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "验证并发现服务" })).toBeInTheDocument();
  });

  it("rejects an invalid SSH alias before calling the backend", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    const alias = await screen.findByLabelText(/主机别名/);
    fireEvent.change(alias, { target: { value: "bad alias!" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并发现服务" }));
    expect(await screen.findByText(/SSH 别名格式无效/)).toBeInTheDocument();
  });

  it("surfaces SSH validation errors with the desktop error code", async () => {
    class SshRejectAdapter extends ConnectorAdapter {
      override async validateSshConnection(): Promise<SshValidateResult> { throw { code: "ssh_host_key_mismatch" }; }
    }
    render(<DesktopAdapterProvider adapter={new SshRejectAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    const alias = await screen.findByLabelText(/主机别名/);
    fireEvent.change(alias, { target: { value: "fixture-host" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并发现服务" }));
    expect(await screen.findByText(/ssh_host_key_mismatch/)).toBeInTheDocument();
  });

  it("discovers SSH services and creates a scoped connection", async () => {
    class SshServicesAdapter extends ConnectorAdapter {
      override async validateSshConnection() {
        return { discovered_services: [{ id: "fixture-service-1", name: "fixture-launchd" }, { id: "fixture-service-2", name: "fixture-systemd" }] };
      }
      override async createSshConnection() { return { connection_id: "fixture-ssh-conn", sync_run_id: "fixture-ssh-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new SshServicesAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    fireEvent.change(await screen.findByLabelText("SSH 连接名称"), { target: { value: "Mac Mini" } });
    fireEvent.change(screen.getByLabelText(/主机别名/), { target: { value: "mac-mini" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并发现服务" }));
    expect(await screen.findByLabelText("fixture-launchd")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("fixture-launchd"));
    fireEvent.click(screen.getByRole("button", { name: "创建连接并同步 1 个服务" }));
    expect(await screen.findByText(/SSH 连接已创建，将在后台同步 1 个服务/)).toBeInTheDocument();
  });

  it("allows creating an SSH connection with zero discovered services", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    fireEvent.change(await screen.findByLabelText("SSH 连接名称"), { target: { value: "Empty" } });
    fireEvent.change(screen.getByLabelText(/主机别名/), { target: { value: "empty-host" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并发现服务" }));
    expect(await screen.findByText(/没有可用的服务，仍可创建空范围连接/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建连接并同步 0 个服务" }));
    expect(await screen.findByText(/SSH 连接已创建，将在后台同步 0 个服务/)).toBeInTheDocument();
  });

  it("renders the Dokploy connection form section", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByRole("heading", { name: "添加 Dokploy 连接" })).toBeInTheDocument();
    expect(screen.getByLabelText("Dokploy 连接名称")).toBeInTheDocument();
    expect(screen.getByLabelText(/实例 URL/)).toBeInTheDocument();
    expect(screen.getByLabelText("API Token")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "验证并统计项目" })).toBeInTheDocument();
  });

  it("rejects an invalid Dokploy URL before calling the backend", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    const url = await screen.findByLabelText(/实例 URL/);
    fireEvent.change(url, { target: { value: "not-a-url" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/Dokploy 实例 URL 无效/)).toBeInTheDocument();
  });

  it("surfaces Dokploy validation errors with the desktop error code", async () => {
    class DokployRejectAdapter extends ConnectorAdapter {
      override async validateDokployConnection(): Promise<DokployValidateResult> { throw { code: "dokploy_auth_failed" }; }
    }
    render(<DesktopAdapterProvider adapter={new DokployRejectAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/dokploy_auth_failed/)).toBeInTheDocument();
  });

  it("validates a Dokploy instance and creates a scoped connection", async () => {
    class DokployOkAdapter extends ConnectorAdapter {
      override async validateDokployConnection() { return { project_count: 3 }; }
      override async createDokployConnection() { return { connection_id: "fixture-dokploy-conn", sync_run_id: "fixture-dokploy-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new DokployOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/已验证 Dokploy 实例，发现 3 个项目/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接并同步" })).toBeEnabled();
    fireEvent.submit(screen.getByRole("button", { name: "创建连接并同步" }).closest("form")!);
    expect(await screen.findByText(/Dokploy 连接已创建，将在后台同步：fixture-dokploy-sync/)).toBeInTheDocument();
  });
});
