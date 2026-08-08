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
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
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
      await screen.findByRole("heading", { level: 1, name: "概览" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { level: 2, name: "关注队列" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("已保存事实已过期")).not.toHaveLength(0);
    expect(screen.queryByRole("heading", { name: "Goal 1 placeholder" })).not.toBeInTheDocument();
    const runtime = screen.getByRole("contentinfo", { name: "控制平面运行时" });
    expect(within(runtime).getByText("Goal 3 查询界面")).toBeInTheDocument();
    expect(within(runtime).getByText("已禁用提供方写入")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "概览" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("exposes all six accessible routes and tracks the active state", async () => {
    const user = userEvent.setup();
    renderShell();

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    for (const label of [
      "概览", "资源清单", "拓扑", "时间线", "连接器", "设置",
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

  it("renders Timeline as a committed change surface", async () => {
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByRole("button", { name: "时间线" }));

    expect(
      await screen.findByRole("heading", { level: 1, name: "时间线" }),
    ).toBeInTheDocument();
    expect(screen.getByText("没有已持久化的变更。")).toBeInTheDocument();
  });

  it("switches the color theme from the Settings page and persists the preference", async () => {
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("heading", { level: 1, name: "设置" });

    await user.click(await screen.findByRole("button", { name: "暗色" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(window.localStorage.getItem("theme")).toBe("light");

    await user.click(screen.getByRole("button", { name: "亮色" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("theme")).toBe("dark");
  });

  it("closes and reopens the Inspector while releasing its desktop column", async () => {
    const user = userEvent.setup();
    const { container } = renderShell();

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    await user.click(within(inspector).getByRole("button", { name: "关闭检查器" }));

    expect(inspector).toHaveAttribute("hidden");
    expect(container.firstElementChild).toHaveClass("inspector-closed");

    await user.click(screen.getByRole("button", { name: "打开检查器" }));
    expect(inspector).not.toHaveAttribute("hidden");
    expect(container.firstElementChild).not.toHaveClass("inspector-closed");
  });

  it("closes the Inspector for Settings and preserves that closed state", async () => {
    const user = userEvent.setup();
    const { container } = renderShell();

    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "设置" }),
    ).toBeInTheDocument();
    const inspector = screen.getByLabelText("证据检查器");
    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "打开检查器" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");

    await user.click(screen.getByRole("button", { name: "概览" }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "概览" }),
    ).toBeInTheDocument();
    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "打开检查器" })).toBeInTheDocument();
  });

  it("focuses the writable search with Meta+K and Control+K", () => {
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "搜索本地基础设施",
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
      name: "搜索本地基础设施",
    });

    await user.type(search, "alpha");
    const results = await screen.findByRole("listbox", { name: "搜索结果" });
    expect(within(results).getAllByRole("option")).not.toHaveLength(0);
    expect(search).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(search, { key: "Escape" });

    await waitFor(() => {
      expect(search).toHaveValue("");
      expect(search).not.toHaveFocus();
      expect(search).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByRole("listbox", { name: "搜索结果" })).not.toBeInTheDocument();
    });
  });

  it("selects a bounded search result into Inventory detail and opens its resource Inspector", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "搜索本地基础设施",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );

    expect(
      await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "资源清单" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(search).toHaveValue("");
    expect(screen.queryByRole("listbox", { name: "搜索结果" })).not.toBeInTheDocument();

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    await waitFor(() => {
      expect(
        within(inspector).getByRole("heading", {
          level: 3,
          name: "Fixture Compute Alpha",
        }),
      ).toBeInTheDocument();
    });
    expect(within(inspector).getByText("资源")).toBeInTheDocument();
  });

  it("remembers a selected resource as a reachable bounded Topology focus", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "搜索本地基础设施",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );
    await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" });

    await user.click(screen.getByRole("button", { name: "拓扑" }));

    expect(
      await screen.findByRole("heading", { level: 1, name: "拓扑" }),
    ).toBeInTheDocument();
    expect(await screen.findByLabelText("受限关系边")).toBeInTheDocument();
    expect(await screen.findByText("fixture-resource-alpha")).toBeInTheDocument();
    expect(screen.getByText("受限")).toBeInTheDocument();
  });

  it("routes topology node and relation selection into the Inspector", async () => {
    const user = userEvent.setup();
    renderShell();
    const search = screen.getByRole("combobox", {
      name: "搜索本地基础设施",
    });

    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );
    await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" });
    await user.click(screen.getByRole("button", { name: "拓扑" }));

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
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
        name: "提供方关系 fixture.depends_on",
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

  it("keeps the connection dialog and its draft open across window focus", async () => {
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByRole("button", { name: "连接器" }));
    await user.click(await screen.findByRole("button", { name: "添加连接" }));
    await user.click(await screen.findByRole("button", { name: /^GitHub/ }));
    const name = await screen.findByLabelText("连接名称");
    await user.type(name, "Focus Draft");

    window.dispatchEvent(new Event("focus"));

    expect(screen.getByRole("dialog", { name: "添加连接" })).toBeInTheDocument();
    expect(screen.getByLabelText("连接名称")).toHaveValue("Focus Draft");
  });

  it("re-queries on window focus and invalidation, then unsubscribes on unmount", async () => {
    const adapter = new TrackingAdapter(createQueryEvidenceLifecycleSnapshotFixture());
    const { unmount } = renderShell(adapter);

    expect(await screen.findAllByText("已保存事实已过期")).not.toHaveLength(0);
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
    const inspector = screen.getByLabelText("证据检查器");

    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "打开检查器" })).toBeInTheDocument();
    expect(container.firstElementChild).toHaveClass("inspector-closed");
  });
});
