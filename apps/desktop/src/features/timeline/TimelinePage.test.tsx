import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { TimelinePageDto } from "../../generated/query/TimelinePageDto";
import type { TimelineInput } from "../../platform/desktop-adapter/desktop-adapter";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createEmptyQuerySnapshotFixture } from "../../test/fixtures/query-fixtures";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import {
  createSplitGroupTimelinePagesFixture,
  createTimelinePageFixture,
} from "../../test/fixtures/timeline-fixtures";
import { TimelinePage } from "./TimelinePage";

afterEach(cleanup);

/** Serves an ordered page list, one page per getTimeline call. */
class SequentialTimelineAdapter extends MockDesktopAdapter {
  readonly requests: TimelineInput[] = [];
  failRequestIndex: number | null = null;

  constructor(private readonly pages: readonly TimelinePageDto[]) {
    super(createEmptyQuerySnapshotFixture());
  }

  override async getTimeline(input: TimelineInput = {}) {
    this.requests.push(input);
    if (this.failRequestIndex === this.requests.length - 1) {
      throw new Error("fixture-transport-failure");
    }
    return this.pages[Math.min(this.requests.length - 1, this.pages.length - 1)];
  }
}

/** Lets the test resolve each in-flight request out of order. */
class DeferredTimelineAdapter extends MockDesktopAdapter {
  readonly requests: TimelineInput[] = [];
  private readonly resolvers: Array<() => void> = [];

  constructor(private readonly pages: readonly TimelinePageDto[]) {
    super(createEmptyQuerySnapshotFixture());
  }

  override async getTimeline(input: TimelineInput = {}) {
    this.requests.push(input);
    const index = this.requests.length - 1;
    const page = this.pages[Math.min(index, this.pages.length - 1)];
    return new Promise<TimelinePageDto>((resolve) => {
      this.resolvers.push(() => resolve(page));
    });
  }

  resolveRequest(index: number) {
    this.resolvers[index]?.();
  }
}

function renderPage(adapter: MockDesktopAdapter, queryVersion = 0) {
  return render(
    <DesktopAdapterProvider adapter={adapter}>
      <TimelinePage queryVersion={queryVersion} />
    </DesktopAdapterProvider>,
  );
}

describe("TimelinePage", () => {
  it("renders the empty state with a hint when nothing is persisted", async () => {
    renderPage(new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture()));

    expect(
      await screen.findByText("没有已持久化的变更。"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("完成一次同步或建立绑定后，审计记录会出现在这里。"),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "时间线" })).toBeInTheDocument();
  });

  it("renders groups, subject identities, scalar diffs, and nested JSON diffs", async () => {
    renderPage(
      new SequentialTimelineAdapter([createTimelinePageFixture()]),
    );

    expect(
      await screen.findByLabelText("同步运行 fixture-sync-run-complete"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("绑定 fixture-binding-alpha-beta")).toBeInTheDocument();
    expect(screen.getByLabelText("推断 fixture-rule-v1")).toBeInTheDocument();

    expect(screen.getAllByText("fixture-resource-alpha").length).toBeGreaterThan(0);
    expect(screen.getByText("fixture-relation-provider-alpha-beta")).toBeInTheDocument();

    expect(screen.getAllByText("变更前").length).toBeGreaterThan(0);
    expect(screen.getAllByText("变更后").length).toBeGreaterThan(0);
    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getByText("null")).toBeInTheDocument();

    // Nested object diff renders as pretty-printed JSON, not a truncated blob.
    expect(screen.getByText(/fixture-tier-standard/)).toBeInTheDocument();
    expect(screen.getByText(/fixture-flag-a/)).toBeInTheDocument();

    expect(screen.getAllByText("fixture-resource-version-alpha-3")).not.toHaveLength(0);
  });

  it("merges a group that the backend split across page boundaries", async () => {
    const [first, second] = createSplitGroupTimelinePagesFixture();
    const user = userEvent.setup();
    renderPage(new SequentialTimelineAdapter([first, second]));

    expect(
      await screen.findByLabelText("同步运行 fixture-sync-run-split"),
    ).toBeInTheDocument();
    expect(screen.getByText("50 项")).toBeInTheDocument();
    expect(screen.getByText("已加载 50 项变更")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "加载更多" }));

    // The continuation of the split group is merged into one section only.
    expect(screen.getAllByLabelText("同步运行 fixture-sync-run-split")).toHaveLength(1);
    expect(await screen.findByText("52 项")).toBeInTheDocument();
    expect(screen.getByLabelText("绑定 fixture-binding-split")).toBeInTheDocument();
    expect(screen.getByText("已加载 53 项变更")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "加载更多" })).not.toBeInTheDocument();
  });

  it("keeps loaded content and shows an inline hint when load-more fails", async () => {
    const [first, second] = createSplitGroupTimelinePagesFixture();
    const adapter = new SequentialTimelineAdapter([first, second]);
    adapter.failRequestIndex = 1;
    const user = userEvent.setup();
    renderPage(adapter);

    await screen.findByText("50 项");

    await user.click(screen.getByRole("button", { name: "加载更多" }));

    expect(await screen.findByText("无法加载更多变更。")).toBeInTheDocument();
    expect(screen.getByText("50 项")).toBeInTheDocument();
    expect(screen.queryByText("无法读取本地变更时间线。")).not.toBeInTheDocument();

    // A second attempt recovers without a full reload.
    await user.click(screen.getByRole("button", { name: "加载更多" }));
    expect(await screen.findByText("52 项")).toBeInTheDocument();
    expect(screen.queryByText("无法加载更多变更。")).not.toBeInTheDocument();
  });

  it("shows an initial-load failure with a retry that recovers", async () => {
    const adapter = new SequentialTimelineAdapter([createTimelinePageFixture()]);
    adapter.failRequestIndex = 0;
    const user = userEvent.setup();
    renderPage(adapter);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("无法读取本地变更时间线。");
    expect(screen.queryByText("没有已持久化的变更。")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "重试" }));

    expect(
      await screen.findByLabelText("同步运行 fixture-sync-run-complete"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("discards a stale response that resolves after a fresher query", async () => {
    const firstPage: TimelinePageDto = {
      metadata: createTimelinePageFixture().metadata,
      groups: [
        {
          group_id: "fixture-timeline-group-race-a",
          origin: { type: "sync_run", sync_run_id: "fixture-sync-run-race-a" },
          occurred_at: "2000-01-01T00:00:00Z",
          items: [
            {
              change: {
                change_id: "fixture-change-race-a",
                subject: { type: "resource", resource_id: "fixture-resource-race-a" },
                observed_at: "2000-01-01T00:00:00Z",
                fields: [],
                origin: { type: "sync_run", sync_run_id: "fixture-sync-run-race-a" },
              },
              version_links: [],
            },
          ],
        },
      ],
      page_info: { next_cursor: null },
    };
    const secondPage: TimelinePageDto = {
      ...firstPage,
      groups: [
        {
          ...firstPage.groups[0],
          group_id: "fixture-timeline-group-race-b",
          origin: { type: "sync_run", sync_run_id: "fixture-sync-run-race-b" },
          items: [
            {
              change: {
                change_id: "fixture-change-race-b",
                subject: { type: "resource", resource_id: "fixture-resource-race-b" },
                observed_at: "2000-01-01T00:00:00Z",
                fields: [],
                origin: { type: "sync_run", sync_run_id: "fixture-sync-run-race-b" },
              },
              version_links: [],
            },
          ],
        },
      ],
    };
    const adapter = new DeferredTimelineAdapter([firstPage, secondPage]);
    const { rerender } = renderPage(adapter, 0);

    // queryVersion bumps while the first request is still in flight.
    rerender(
      <DesktopAdapterProvider adapter={adapter}>
        <TimelinePage queryVersion={1} />
      </DesktopAdapterProvider>,
    );
    expect(adapter.requests).toHaveLength(2);

    adapter.resolveRequest(1);
    expect(await screen.findByText("fixture-change-race-b")).toBeInTheDocument();

    // The older response must not clobber the fresher state. waitFor retries
    // until the stale promise's continuation AND React's re-render have both
    // flushed, so this assertion genuinely fails if the guard is removed.
    adapter.resolveRequest(0);
    await waitFor(() =>
      expect(screen.queryByText("fixture-change-race-a")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("fixture-change-race-b")).toBeInTheDocument();
    expect(screen.getByText("已加载 1 项变更")).toBeInTheDocument();
    // The stale .finally must not leave the page stuck in the loading state.
    expect(document.querySelector(".timeline-scroll[aria-busy='true']")).toBeNull();
  });

  it("falls back to safe labels and classes for malformed DTO types", async () => {
    const malformed: TimelinePageDto = {
      metadata: createTimelinePageFixture().metadata,
      groups: [
        {
          group_id: "fixture-timeline-group-malformed",
          origin: { type: "widget", widget_id: "nope" } as never,
          occurred_at: "2000-01-01T00:00:00Z",
          items: [
            {
              change: {
                change_id: "fixture-change-malformed",
                subject: { type: "gizmo", gizmo_id: "nope" } as never,
                observed_at: "2000-01-01T00:00:00Z",
                fields: [],
                origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
              },
              version_links: [],
            },
          ],
        },
      ],
      page_info: { next_cursor: null },
    };

    renderPage(new SequentialTimelineAdapter([malformed]));

    expect(await screen.findByLabelText("未知来源")).toBeInTheDocument();
    expect(screen.getByText("未知")).toBeInTheDocument();
    // Class names must come from the closed label map, not the raw DTO type.
    expect(screen.getByText("未知来源")).not.toHaveClass("timeline-origin-dot--widget");
    expect(screen.getByText("未知")).not.toHaveClass("timeline-subject-badge--gizmo");
  });

  it("renders a fallback when a diff value cannot be serialized", async () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    const page: TimelinePageDto = {
      metadata: createTimelinePageFixture().metadata,
      groups: [
        {
          group_id: "fixture-timeline-group-cyclic",
          origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
          occurred_at: "2000-01-01T00:00:00Z",
          items: [
            {
              change: {
                change_id: "fixture-change-cyclic",
                subject: { type: "resource", resource_id: "fixture-resource-alpha" },
                observed_at: "2000-01-01T00:00:00Z",
                fields: [{ path: "attributes.state", before: cyclic, after: "ready" }],
                origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
              },
              version_links: [],
            },
          ],
        },
      ],
      page_info: { next_cursor: null },
    };

    renderPage(new SequentialTimelineAdapter([page]));

    expect(await screen.findByText("（无法序列化）")).toBeInTheDocument();
    expect(screen.getByText("ready")).toBeInTheDocument();
  });

  it("shows load-more instead of the empty state for an empty page with a cursor", async () => {
    const emptyPage: TimelinePageDto = {
      metadata: createTimelinePageFixture().metadata,
      groups: [],
      page_info: { next_cursor: "fixture-cursor-empty" as never },
    };
    const user = userEvent.setup();
    renderPage(new SequentialTimelineAdapter([emptyPage]));

    expect(await screen.findByRole("button", { name: "加载更多" })).toBeInTheDocument();
    expect(screen.queryByText("没有已持久化的变更。")).not.toBeInTheDocument();
  });

  it("clears a stale load-more error when a fresh query reload succeeds", async () => {
    const [first, second] = createSplitGroupTimelinePagesFixture();
    const adapter = new SequentialTimelineAdapter([first, second]);
    adapter.failRequestIndex = 1;
    const user = userEvent.setup();
    const { rerender } = renderPage(adapter, 0);

    await screen.findByText("50 项");
    await user.click(screen.getByRole("button", { name: "加载更多" }));
    expect(await screen.findByText("无法加载更多变更。")).toBeInTheDocument();

    // A queryVersion bump triggers a fresh initial load that replaces the
    // groups and must clear the previous load-more error once it succeeds.
    rerender(
      <DesktopAdapterProvider adapter={adapter}>
        <TimelinePage queryVersion={1} />
      </DesktopAdapterProvider>,
    );
    expect(await screen.findByText("2 项")).toBeInTheDocument();
    expect(screen.queryByText("无法加载更多变更。")).not.toBeInTheDocument();
  });
});
