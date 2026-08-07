import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { SettingsPage } from "./SettingsPage";

afterEach(cleanup);

describe("SettingsPage", () => {
  it("keeps start-at-login and MCP auto-launch separate", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><SettingsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("登录时启动")).toBeInTheDocument();
    expect(screen.getByText("MCP 自动启动")).toBeInTheDocument();
    expect(screen.getByText("不可用")).toBeInTheDocument();
    expect(screen.getByText(/尚未安装、启用或验证/)).toBeInTheDocument();
    expect(screen.getByText("未锁定")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /MCP/i })).not.toBeInTheDocument();
  });

  it("updates local start-at-login state without secret controls", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><SettingsPage /></DesktopAdapterProvider>);
    const toggle = await screen.findByRole("button", { name: "关闭" });
    fireEvent.click(toggle);
    expect(await screen.findByRole("button", { name: "开启" })).toHaveAttribute("aria-pressed", "true");
    expect(document.body.textContent).not.toContain("fixture-binding-alpha-beta");
  });

  it("renders explicit Quit as guidance-only suppression", async () => {
    const adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    const settings = await adapter.getLocalSettings();
    await adapter.updateLocalSettings({ ...settings, user_quit: true });
    render(<DesktopAdapterProvider adapter={adapter}><SettingsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("已锁定")).toBeInTheDocument();
    expect(screen.getByText(/请交互式重新打开 Next Infra/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear/i })).not.toBeInTheDocument();
  });
});
