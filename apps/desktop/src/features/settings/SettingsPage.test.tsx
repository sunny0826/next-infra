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
    expect(await screen.findByText("Start at login")).toBeInTheDocument();
    expect(screen.getByText("MCP auto-launch")).toBeInTheDocument();
    expect(screen.getByText("unavailable")).toBeInTheDocument();
    expect(screen.getByText(/not installed, enabled, or verified/)).toBeInTheDocument();
    expect(screen.getByText("clear")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /MCP/i })).not.toBeInTheDocument();
  });

  it("updates local start-at-login state without secret controls", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><SettingsPage /></DesktopAdapterProvider>);
    const toggle = await screen.findByRole("button", { name: "Off" });
    fireEvent.click(toggle);
    expect(await screen.findByRole("button", { name: "On" })).toHaveAttribute("aria-pressed", "true");
    expect(document.body.textContent).not.toContain("fixture-binding-alpha-beta");
  });

  it("renders explicit Quit as guidance-only suppression", async () => {
    const adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    const settings = await adapter.getLocalSettings();
    await adapter.updateLocalSettings({ ...settings, user_quit: true });
    render(<DesktopAdapterProvider adapter={adapter}><SettingsPage /></DesktopAdapterProvider>);
    expect(await screen.findByText("latched")).toBeInTheDocument();
    expect(screen.getByText(/Reopen Next Infra interactively/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear/i })).not.toBeInTheDocument();
  });
});
