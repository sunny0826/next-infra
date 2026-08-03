import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppShell } from "./app/AppShell";
import { DesktopAdapterProvider } from "./platform/desktop-adapter/DesktopAdapterContext";
import type {
  QueryInvalidation,
  SearchResourcesInput,
  Unsubscribe,
} from "./platform/desktop-adapter/desktop-adapter";
import { MockDesktopAdapter } from "./platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "./test/fixtures/query-fixtures";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

class TrackingAdapter extends MockDesktopAdapter {
  readonly searchRequests: SearchResourcesInput[] = [];
  invalidationListener: ((invalidation: QueryInvalidation) => void) | null = null;
  unsubscribed = false;

  override async searchResources(input: SearchResourcesInput = {}) {
    this.searchRequests.push(input);
    return super.searchResources(input);
  }

  override async subscribeInvalidations(
    listener: (invalidation: QueryInvalidation) => void,
  ): Promise<Unsubscribe> {
    this.invalidationListener = listener;
    return () => {
      this.unsubscribed = true;
    };
  }

  emitInvalidation() {
    this.invalidationListener?.({
      version: "fixture-invalidation-v1",
      scopes: ["resources"],
    });
  }
}

function renderShell(adapter = new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())) {
  return render(
    <DesktopAdapterProvider adapter={adapter}>
      <AppShell />
    </DesktopAdapterProvider>,
  );
}

describe("React app shell", () => {
  it("renders Overview as the default integrated page", async () => {
    renderShell();

    expect(
      await screen.findByRole("heading", { level: 1, name: "Overview" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { level: 2, name: "Attention queue" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Saved fact is expired")).not.toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Goal 1 placeholder" })).not.toBeInTheDocument();
    const runtime = screen.getByRole("contentinfo", { name: "Control Plane Runtime" });
    expect(within(runtime).getByText("Goal 3 query surface")).toBeInTheDocument();
    expect(within(runtime).getByText("provider writes disabled")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Overview" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("exposes all six accessible routes and tracks the active state", async () => {
    const user = userEvent.setup();
    renderShell();

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    for (const label of [
      "Overview",
      "Inventory",
      "Topology",
      "Timeline",
      "Connectors",
      "Settings",
    ]) {
      const button = within(navigation).getByRole("button", { name: label });
      await user.click(button);
      expect(
        await screen.findByRole("heading", { level: 1, name: label }),
      ).toBeInTheDocument();
      expect(button).toHaveAttribute("aria-current", "page");
      expect(
        within(navigation)
          .getAllByRole("button")
          .filter((candidate) => candidate.getAttribute("aria-current") === "page"),
      ).toHaveLength(1);
    }
  });

  it("marks Timeline as explicitly unavailable until Goal 7", async () => {
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByRole("button", { name: "Timeline" }));

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Timeline unavailable until Goal 7",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("This route is intentionally unavailable, not an empty query result."),
    ).toBeInTheDocument();
  });

  it("closes and reopens the Inspector while releasing its desktop column", async () => {
    const user = userEvent.setup();
    const { container } = renderShell();

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
    const { container } = renderShell();

    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "Settings" }),
    ).toBeInTheDocument();
    const inspector = screen.getByLabelText("Evidence inspector");
    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Open inspector" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");

    await user.click(screen.getByRole("button", { name: "Overview" }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "Overview" }),
    ).toBeInTheDocument();
    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Open inspector" })).toBeInTheDocument();
  });

  it("focuses the writable search with Meta+K and Control+K", () => {
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });

    expect(search).not.toHaveAttribute("readonly");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(search).toHaveFocus();

    fireEvent.blur(search);
    fireEvent.keyDown(window, { key: "K", ctrlKey: true });
    expect(search).toHaveFocus();
  });

  it("clears and closes search results with Escape", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });

    await user.type(search, "alpha");
    const results = await screen.findByRole("listbox", { name: "Search results" });
    expect(within(results).getAllByRole("option")).not.toHaveLength(0);
    expect(search).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(search, { key: "Escape" });

    await waitFor(() => {
      expect(search).toHaveValue("");
      expect(search).not.toHaveFocus();
      expect(search).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByRole("listbox", { name: "Search results" })).not.toBeInTheDocument();
    });
  });

  it("selects a bounded search result into Inventory detail and opens its resource Inspector", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );

    expect(
      await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Inventory" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(search).toHaveValue("");
    expect(screen.queryByRole("listbox", { name: "Search results" })).not.toBeInTheDocument();

    const inspector = screen.getByRole("complementary", { name: "Evidence inspector" });
    await waitFor(() => {
      expect(
        within(inspector).getByRole("heading", {
          level: 3,
          name: "Fixture Compute Alpha",
        }),
      ).toBeInTheDocument();
    });
    expect(within(inspector).getByText("Resource")).toBeInTheDocument();
  });

  it("remembers a selected resource as a reachable bounded Topology focus", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );
    await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" });

    await user.click(screen.getByRole("button", { name: "Topology" }));

    expect(
      await screen.findByRole("heading", { level: 1, name: "Topology" }),
    ).toBeInTheDocument();
    expect(await screen.findByLabelText("Bounded relation edges")).toBeInTheDocument();
    expect(await screen.findByText("fixture-resource-alpha")).toBeInTheDocument();
    expect(screen.getByText("bounded")).toBeInTheDocument();
  });

  it("routes topology node and relation selection into the Inspector", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );
    await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" });
    await user.click(screen.getByRole("button", { name: "Topology" }));

    const inspector = screen.getByRole("complementary", { name: "Evidence inspector" });
    await user.click(await screen.findByRole("button", { name: /Fixture Database Beta/ }));
    await waitFor(() => {
      expect(
        within(inspector).getByRole("heading", {
          level: 3,
          name: "Fixture Database Beta",
        }),
      ).toBeInTheDocument();
    });

    await user.click(
      await screen.findByRole("button", {
        name: "provider relation fixture.depends_on",
      }),
    );
    await waitFor(() => {
      expect(
        within(inspector).getByRole("heading", {
          level: 3,
          name: "fixture.depends_on",
        }),
      ).toBeInTheDocument();
    });
    expect(within(inspector).getByText("fixture-sync-run-complete")).toBeInTheDocument();
  });

  it("re-queries on window focus and invalidation, then unsubscribes on unmount", async () => {
    const adapter = new TrackingAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    const { unmount } = renderShell(adapter);

    expect(await screen.findAllByText("Saved fact is expired")).not.toHaveLength(0);
    await waitFor(() => expect(adapter.invalidationListener).not.toBeNull());

    const initialSearchCount = adapter.searchRequests.length;
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(adapter.searchRequests.length).toBeGreaterThan(initialSearchCount));

    const focusSearchCount = adapter.searchRequests.length;
    adapter.emitInvalidation();
    await waitFor(() => expect(adapter.searchRequests.length).toBeGreaterThan(focusSearchCount));

    unmount();
    expect(adapter.unsubscribed).toBe(true);
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

    const { container } = renderShell();
    const inspector = screen.getByLabelText("Evidence inspector");

    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Open inspector" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");
  });
});
