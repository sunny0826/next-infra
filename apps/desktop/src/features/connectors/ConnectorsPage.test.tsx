import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import type { SshValidateResult, DokployValidateResult } from "../../platform/desktop-adapter/desktop-adapter";
import { createConnectorCoverageFixtures, createGitHubGoal5SnapshotFixture, createGoal9ConnectorCoverageFixtures, createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { createSyncRunSnapshotFixture } from "../../test/fixtures/sync-run-fixture";
import { ConnectorsPage } from "./ConnectorsPage";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

async function openProviderForm(providerName: string) {
  fireEvent.click(await screen.findByRole("button", { name: "添加连接" }));
  fireEvent.click(await screen.findByRole("button", { name: new RegExp(`^${providerName}`) }));
}

class ConnectorAdapter extends MockDesktopAdapter {
  override async listConnectorCoverage() { return { metadata: (await this.searchResources()).metadata, items: [...createConnectorCoverageFixtures(), ...createGoal9ConnectorCoverageFixtures()] }; }
}

class SyncRunAdapter extends ConnectorAdapter {
  constructor() {
    super(createSyncRunSnapshotFixture());
  }
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
    await openProviderForm("GitHub");
    const name = await screen.findByLabelText("连接名称");
    const token = screen.getByLabelText("细粒度 Token");
    fireEvent.change(name, { target: { value: "Personal GitHub" } });
    fireEvent.change(token, { target: { value: "test-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并加载仓库" }));
    fireEvent.click(await screen.findByLabelText("fixture/first-repository"));
    fireEvent.click(screen.getByRole("button", { name: "创建连接并同步 1 个仓库" }));

    expect(await screen.findByText(/GitHub 连接已创建，将在后台同步 1 个选定仓库/)).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("clears the in-memory token when the dialog is closed without creating", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("GitHub");
    const token = await screen.findByLabelText("细粒度 Token");
    fireEvent.change(token, { target: { value: "test-token" } });
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    // Reopening the dialog must start with an empty token (explicit closeConnectorDialog clearing)
    await openProviderForm("GitHub");
    expect(screen.getByLabelText("细粒度 Token")).toHaveValue("");
  });

  it("clears the in-memory token when navigating back to the provider picker", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("GitHub");
    const token = await screen.findByLabelText("细粒度 Token");
    fireEvent.change(token, { target: { value: "test-token" } });
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    await openProviderForm("GitHub");
    expect(screen.getByLabelText("细粒度 Token")).toHaveValue("");
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
    await openProviderForm("SSH");
    expect(await screen.findByRole("heading", { name: "添加 SSH 连接" })).toBeInTheDocument();
    expect(screen.getByLabelText("SSH 连接名称")).toBeInTheDocument();
    expect(screen.getByLabelText(/主机别名/)).toBeInTheDocument();
    expect(screen.getByLabelText("连接超时（秒）")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "验证并发现服务" })).toBeInTheDocument();
  });

  it("rejects an invalid SSH alias before calling the backend", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("SSH");
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
    await openProviderForm("SSH");
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
    await openProviderForm("SSH");
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
    await openProviderForm("SSH");
    fireEvent.change(await screen.findByLabelText("SSH 连接名称"), { target: { value: "Empty" } });
    fireEvent.change(screen.getByLabelText(/主机别名/), { target: { value: "empty-host" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并发现服务" }));
    expect(await screen.findByText(/没有可用的服务，仍可创建空范围连接/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建连接并同步 0 个服务" }));
    expect(await screen.findByText(/SSH 连接已创建，将在后台同步 0 个服务/)).toBeInTheDocument();
  });

  it("renders the Dokploy connection form section", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
    expect(await screen.findByRole("heading", { name: "添加 Dokploy 连接" })).toBeInTheDocument();
    expect(screen.getByLabelText("Dokploy 连接名称")).toBeInTheDocument();
    expect(screen.getByLabelText(/实例 URL/)).toBeInTheDocument();
    expect(screen.getByLabelText("API Token")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "验证并统计项目" })).toBeInTheDocument();
  });

  it("rejects an invalid Dokploy URL before calling the backend", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
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
    await openProviderForm("Dokploy");
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/dokploy_auth_failed/)).toBeInTheDocument();
  });

  it("validates a Dokploy instance and creates a scoped connection", async () => {
    let createdToken = "";
    class DokployOkAdapter extends ConnectorAdapter {
      override async validateDokployConnection() { return { project_count: 3 }; }
      override async createDokployConnection(input: { token: string }) { createdToken = input.token; return { connection_id: "fixture-dokploy-conn", sync_run_id: "fixture-dokploy-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new DokployOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/已验证 Dokploy 实例，发现 3 个项目/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接并同步" })).toBeEnabled();
    fireEvent.submit(screen.getByRole("button", { name: "创建连接并同步" }).closest("form")!);
    expect(await screen.findByText(/Dokploy 连接已创建，将在后台同步：fixture-dokploy-sync/)).toBeInTheDocument();
    expect(createdToken).toBe("fixture-token");
  });

  it("disables Dokploy create after editing a validated field", async () => {
    class DokployOkAdapter extends ConnectorAdapter {
      override async validateDokployConnection() { return { project_count: 3 }; }
      override async createDokployConnection(input: { token: string }) { return { connection_id: "fixture-dokploy-conn", sync_run_id: "fixture-dokploy-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new DokployOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/已验证 Dokploy 实例，发现 3 个项目/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接并同步" })).toBeEnabled();
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token-changed" } });
    expect(screen.getByRole("button", { name: "创建连接并同步" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));
    expect(await screen.findByText(/已验证 Dokploy 实例，发现 3 个项目/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建连接并同步" })).toBeEnabled();
  });

  it("validates and creates a Cloudflare connection", async () => {
    let createdToken = "";
    class CloudflareOkAdapter extends ConnectorAdapter {
      override async validateCloudflareConnection() { return { account_count: 2 }; }
      override async createCloudflareConnection(input: { token: string }) { createdToken = input.token; return { connection_id: "fixture-cf-conn", sync_run_id: "fixture-cf-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new CloudflareOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Cloudflare");
    fireEvent.change(await screen.findByLabelText("Cloudflare 连接名称"), { target: { value: "CF" } });
    fireEvent.change(screen.getByLabelText("Cloudflare Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计账户" }));
    expect(await screen.findByText(/已验证 Cloudflare 账户，发现 2 个账户/)).toBeInTheDocument();
    fireEvent.submit(screen.getByRole("button", { name: "创建 Cloudflare 连接并同步" }).closest("form")!);
    expect(await screen.findByText(/Cloudflare 连接已创建，将在后台同步：fixture-cf-sync/)).toBeInTheDocument();
    expect(createdToken).toBe("fixture-token");
  });

  it("disables create after editing a validated secret and clears secrets on dialog close", async () => {
    class CloudflareOkAdapter extends ConnectorAdapter {
      override async validateCloudflareConnection() { return { account_count: 2 }; }
      override async createCloudflareConnection(input: { token: string }) { return { connection_id: "fixture-cf-conn", sync_run_id: "fixture-cf-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new CloudflareOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Cloudflare");
    fireEvent.change(await screen.findByLabelText("Cloudflare 连接名称"), { target: { value: "CF" } });
    fireEvent.change(screen.getByLabelText("Cloudflare Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计账户" }));
    expect(await screen.findByText(/已验证 Cloudflare 账户，发现 2 个账户/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建 Cloudflare 连接并同步" })).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Cloudflare Token"), { target: { value: "fixture-token-changed" } });
    expect(screen.getByRole("button", { name: "创建 Cloudflare 连接并同步" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("validates and creates a Supabase managed connection", async () => {
    class SupabaseOkAdapter extends ConnectorAdapter {
      override async validateSupabaseManagedConnection() { return { project_count: 4 }; }
      override async createSupabaseManagedConnection() { return { connection_id: "fixture-sb-conn", sync_run_id: "fixture-sb-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new SupabaseOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Supabase");
    fireEvent.change(await screen.findByLabelText("Supabase 连接名称"), { target: { value: "SB" } });
    fireEvent.change(screen.getByLabelText("Supabase Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计 Supabase 项目" }));
    expect(await screen.findByText(/已验证 Supabase 账户，发现 4 个项目/)).toBeInTheDocument();
    fireEvent.submit(screen.getByRole("button", { name: "创建 Supabase 连接并同步" }).closest("form")!);
    expect(await screen.findByText(/Supabase 连接已创建，将在后台同步：fixture-sb-sync/)).toBeInTheDocument();
  });

  it("validates and creates an Aliyun connection with a region", async () => {
    class AliyunOkAdapter extends ConnectorAdapter {
      override async validateAliyunConnection() { return { resource_count: 12 }; }
      override async createAliyunConnection() { return { connection_id: "fixture-ali-conn", sync_run_id: "fixture-ali-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new AliyunOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("阿里云");
    fireEvent.change(await screen.findByLabelText("阿里云连接名称"), { target: { value: "Ali" } });
    fireEvent.change(screen.getByLabelText("阿里云 AccessKey ID"), { target: { value: "fixture-id" } });
    fireEvent.change(screen.getByLabelText("阿里云 AccessKey Secret"), { target: { value: "fixture-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计阿里云资源" }));
    expect(await screen.findByText(/已验证阿里云凭据，发现 12 个资源/)).toBeInTheDocument();
    fireEvent.submit(screen.getByRole("button", { name: "创建阿里云连接并同步" }).closest("form")!);
    expect(await screen.findByText(/阿里云连接已创建，将在后台同步：fixture-ali-sync/)).toBeInTheDocument();
  });

  it("keeps the GitHub draft across dialog close and reopen", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("GitHub");
    fireEvent.change(await screen.findByLabelText("连接名称"), { target: { value: "Draft GitHub" } });
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await openProviderForm("GitHub");
    expect(screen.getByLabelText("连接名称")).toHaveValue("Draft GitHub");
  });

  it("restores non-secret drafts after remount while keeping secrets out of storage", async () => {
    const { unmount } = render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Draft Dokploy" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-draft-token" } });

    unmount();
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");

    expect(screen.getByLabelText("Dokploy 连接名称")).toHaveValue("Draft Dokploy");
    expect(screen.getByLabelText(/实例 URL/)).toHaveValue("https://fixture.example.test");
    expect(screen.getByLabelText("API Token")).toHaveValue("");
  });

  it("shows an in-flight progress indicator on the validate button", async () => {
    class SlowValidateAdapter extends ConnectorAdapter {
      override async validateDokployConnection() {
        await new Promise(() => undefined);
        return { project_count: 3 };
      }
    }
    render(<DesktopAdapterProvider adapter={new SlowValidateAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("Dokploy");
    fireEvent.change(await screen.findByLabelText("Dokploy 连接名称"), { target: { value: "Prod" } });
    fireEvent.change(screen.getByLabelText(/实例 URL/), { target: { value: "https://fixture.example.test" } });
    fireEvent.change(screen.getByLabelText("API Token"), { target: { value: "fixture-token" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计项目" }));

    expect(await screen.findByText("正在验证…")).toBeInTheDocument();
    expect(document.querySelector(".connectors-button-progress")).not.toBeNull();
  });

  it("validates and creates a Tencent connection with a region", async () => {
    class TencentOkAdapter extends ConnectorAdapter {
      override async validateTencentConnection() { return { resource_count: 7 }; }
      override async createTencentConnection() { return { connection_id: "fixture-tc-conn", sync_run_id: "fixture-tc-sync" }; }
    }
    render(<DesktopAdapterProvider adapter={new TencentOkAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ConnectorsPage /></DesktopAdapterProvider>);
    await openProviderForm("腾讯云");
    fireEvent.change(await screen.findByLabelText("腾讯云连接名称"), { target: { value: "TC" } });
    fireEvent.change(screen.getByLabelText("腾讯云 SecretId"), { target: { value: "fixture-id" } });
    fireEvent.change(screen.getByLabelText("腾讯云 SecretKey"), { target: { value: "fixture-key" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并统计腾讯云资源" }));
    expect(await screen.findByText(/已验证腾讯云凭据，发现 7 个资源/)).toBeInTheDocument();
    fireEvent.submit(screen.getByRole("button", { name: "创建腾讯云连接并同步" }).closest("form")!);
    expect(await screen.findByText(/腾讯云连接已创建，将在后台同步：fixture-tc-sync/)).toBeInTheDocument();
  });

  it("expands a connection row into a SyncRun provenance chain", async () => {
    render(<DesktopAdapterProvider adapter={new SyncRunAdapter()}><ConnectorsPage /></DesktopAdapterProvider>);
    const expanders = await screen.findAllByRole("button", { name: "展开" });
    fireEvent.click(expanders[0]);

    const detail = within(document.getElementById("connectors-run-detail-fixture-connection-sync") as HTMLElement);
    expect(detail.getByLabelText("同步来源链")).toBeInTheDocument();
    expect(detail.getByText("连接器")).toBeInTheDocument();
    expect(detail.getByText("连接")).toBeInTheDocument();
    expect(detail.getByText("同步运行")).toBeInTheDocument();
    expect(detail.getByText("覆盖")).toBeInTheDocument();
    expect(detail.getAllByText("fixture-sync-run-failed").length).toBeGreaterThan(0);
    expect(detail.getAllByText("失败").length).toBeGreaterThan(0);
    expect(detail.getByText(/读取 3 · 创建 0 · 更新 0 · 未变 0 · 警告 0/)).toBeInTheDocument();
  });

  it("shows the coverage reason on a partial SyncRun", async () => {
    render(<DesktopAdapterProvider adapter={new SyncRunAdapter()}><ConnectorsPage /></DesktopAdapterProvider>);
    const expanders = await screen.findAllByRole("button", { name: "展开" });
    fireEvent.click(expanders[0]);

    const detail = within(document.getElementById("connectors-run-detail-fixture-connection-sync") as HTMLElement);
    expect(detail.getByText("Fixture: remaining pages skipped after quota limit.")).toBeInTheDocument();
    expect(detail.getByText("fixture-sync-run-partial")).toBeInTheDocument();
    expect(detail.getAllByText("部分覆盖").length).toBeGreaterThan(0);
  });

  it("shows the error message on a failed SyncRun", async () => {
    render(<DesktopAdapterProvider adapter={new SyncRunAdapter()}><ConnectorsPage /></DesktopAdapterProvider>);
    const expanders = await screen.findAllByRole("button", { name: "展开" });
    fireEvent.click(expanders[0]);

    const detail = within(document.getElementById("connectors-run-detail-fixture-connection-sync") as HTMLElement);
    expect(detail.getByText("fixture_auth_expired: Fixture: credential refresh failed.（可重试）")).toBeInTheDocument();
  });

  it("shows the never-synced state for a connection without runs", async () => {
    render(<DesktopAdapterProvider adapter={new SyncRunAdapter()}><ConnectorsPage /></DesktopAdapterProvider>);
    const expanders = await screen.findAllByRole("button", { name: "展开" });
    fireEvent.click(expanders[1]);

    const detail = within(document.getElementById("connectors-run-detail-fixture-connection-never") as HTMLElement);
    expect(detail.getByText("从未成功同步")).toBeInTheDocument();
    expect(detail.getByText("无覆盖记录")).toBeInTheDocument();
    expect(detail.getByText(/该连接从未成功同步。尚无 SyncRun 记录/)).toBeInTheDocument();
  });

  it("shows initial-setup guidance when no connections exist", async () => {
    render(<DesktopAdapterProvider adapter={new ConnectorAdapter({ metadata: { schema_version: 1, snapshot_version: "fixture-empty-v1", generated_at: "2000-01-01T00:00:00Z" }, resources: [], relations: [], connections: [] })}><ConnectorsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText(/尚无本地连接。请在「添加连接」中配置第一个只读连接器/)).toBeInTheDocument();
    expect(screen.getByText("0 个本地连接")).toBeInTheDocument();
  });
});
