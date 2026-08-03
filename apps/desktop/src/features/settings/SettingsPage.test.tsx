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
    expect(screen.getByRole("button", { name: /Unavailable/ })).toBeDisabled();
  });

  it("updates local start-at-login state without secret controls", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><SettingsPage /></DesktopAdapterProvider>);
    const toggle = await screen.findByRole("button", { name: "Off" });
    fireEvent.click(toggle);
    expect(await screen.findByRole("button", { name: "On" })).toHaveAttribute("aria-pressed", "true");
    expect(document.body.textContent).not.toContain("fixture-binding-alpha-beta");
  });
});
