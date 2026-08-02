import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppShell } from "./app/AppShell";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("React app shell", () => {
  it("renders Overview as the default Goal 1 placeholder", () => {
    render(<AppShell />);

    expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Goal 1 placeholder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Overview" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("exposes all six navigation destinations by accessible name", () => {
    render(<AppShell />);

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    for (const label of [
      "Overview",
      "Inventory",
      "Topology",
      "Timeline",
      "Connectors",
      "Settings",
    ]) {
      expect(within(navigation).getByRole("button", { name: label })).toBeInTheDocument();
    }
  });

  it("switches the local route and active state", async () => {
    const user = userEvent.setup();
    render(<AppShell />);

    const timeline = screen.getByRole("button", { name: "Timeline" });
    await user.click(timeline);

    expect(screen.getByRole("heading", { level: 1, name: "Timeline" })).toBeInTheDocument();
    expect(timeline).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Overview" })).not.toHaveAttribute(
      "aria-current",
    );
  });

  it("keeps Inspector and Runtime semantics explicit without query data", () => {
    render(<AppShell />);

    const inspector = screen.getByRole("complementary", { name: "Evidence inspector" });
    expect(within(inspector).getByRole("heading", { name: "No selection" })).toBeInTheDocument();

    const runtime = screen.getByRole("contentinfo", { name: "Control Plane Runtime" });
    expect(within(runtime).getByText("Runtime not connected")).toBeInTheDocument();
    expect(within(runtime).getByText("local · read-only")).toBeInTheDocument();

    expect(screen.getByRole("searchbox", { name: "Search local infrastructure" })).toHaveAttribute(
      "readonly",
    );
  });

  it("closes and reopens the Inspector while releasing its desktop column", async () => {
    const user = userEvent.setup();
    const { container } = render(<AppShell />);

    const inspector = screen.getByRole("complementary", { name: "Evidence inspector" });
    await user.click(within(inspector).getByRole("button", { name: "Close inspector" }));

    expect(inspector).toHaveAttribute("hidden");
    expect(container.firstElementChild).toHaveClass("inspector-closed");

    await user.click(screen.getByRole("button", { name: "Open inspector" }));
    expect(inspector).not.toHaveAttribute("hidden");
    expect(container.firstElementChild).not.toHaveClass("inspector-closed");
  });

  it("closes the Inspector for Settings and preserves that closed state", async () => {
    const user = userEvent.setup();
    const { container } = render(<AppShell />);

    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.queryByRole("complementary", { name: "Evidence inspector" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open inspector" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");

    await user.click(screen.getByRole("button", { name: "Overview" }));
    expect(screen.queryByRole("complementary", { name: "Evidence inspector" })).not.toBeInTheDocument();
  });

  it("focuses the read-only search with Meta+K and Control+K", () => {
    render(<AppShell />);
    const search = screen.getByRole("searchbox", { name: "Search local infrastructure" });

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(search).toHaveFocus();

    search.blur();
    fireEvent.keyDown(window, { key: "K", ctrlKey: true });
    expect(search).toHaveFocus();
  });

  it("starts with the Inspector closed on a narrow viewport", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: query === "(max-width: 1180px)",
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(() => false),
      })),
    );

    const { container } = render(<AppShell />);

    expect(screen.queryByRole("complementary", { name: "Evidence inspector" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open inspector" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");
  });
});
