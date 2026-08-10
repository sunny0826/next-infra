import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { TimelineItemDto } from "../../generated/query/TimelineItemDto";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { FIXTURE_OBSERVED_AT } from "../../test/fixtures/query-fixtures";
import { TimelineEvidenceAdapter } from "../../test/fixtures/timeline-evidence-adapter";
import { TimelineItem } from "./TimelineItem";

afterEach(cleanup);

function changeItem(subject: TimelineItemDto["change"]["subject"]): TimelineItemDto {
  return {
    change: {
      change_id: "fixture-change-evidence",
      subject,
      observed_at: FIXTURE_OBSERVED_AT,
      fields: [],
      origin: { type: "sync_run", sync_run_id: "fixture-sync-run-complete" },
    },
    version_links: [],
  };
}

function renderItem(adapter: TimelineEvidenceAdapter, item: TimelineItemDto) {
  return render(
    <DesktopAdapterProvider adapter={adapter}>
      <TimelineItem item={item} />
    </DesktopAdapterProvider>,
  );
}

describe("TimelineItem evidence expander", () => {
  it("stays collapsed until the summary is opened", () => {
    renderItem(
      new TimelineEvidenceAdapter(),
      changeItem({ type: "resource", resource_id: "fixture-resource-alpha" }),
    );

    expect(screen.getByText("证据链")).toBeInTheDocument();
    expect(screen.queryByText("当前事实")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "证据链" }),
    ).not.toBeInTheDocument();
  });

  it("expands a resource-subject change into an evidence spine with all three rows", async () => {
    const user = userEvent.setup();
    renderItem(
      new TimelineEvidenceAdapter(),
      changeItem({ type: "resource", resource_id: "fixture-resource-alpha" }),
    );

    await user.click(screen.getByText("证据链"));

    expect(
      await screen.findByRole("heading", { name: "证据链" }),
    ).toBeInTheDocument();
    // The snapshot holds three alpha↔beta relations, all in one spine.
    expect(screen.getByText("3 个来源")).toBeInTheDocument();
    expect(screen.getByLabelText("提供方 evidence")).toBeInTheDocument();
    expect(screen.getByLabelText("已配置 evidence")).toBeInTheDocument();
    expect(screen.getByLabelText("推断 evidence")).toBeInTheDocument();
    // Both endpoints are resolved to full ResourceDto facts.
    expect(screen.getByText("Fixture Compute Alpha")).toBeInTheDocument();
    expect(screen.getByText("Fixture Database Beta")).toBeInTheDocument();
  });

  it("shows the binding explanation without a spine", async () => {
    const user = userEvent.setup();
    renderItem(
      new TimelineEvidenceAdapter(),
      changeItem({ type: "binding", binding_id: "fixture-binding-alpha-beta" }),
    );

    await user.click(screen.getByText("证据链"));

    expect(
      await screen.findByText("此变更源于绑定，缺少可解析的关系端点。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "证据链" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("当前事实")).not.toBeInTheDocument();
  });

  it("shows the relation explanation without a spine", async () => {
    const user = userEvent.setup();
    renderItem(
      new TimelineEvidenceAdapter(),
      changeItem({
        type: "relation",
        relation_id: "fixture-relation-provider-alpha-beta",
      }),
    );

    await user.click(screen.getByText("证据链"));

    expect(
      await screen.findByText("此变更涉及关系，缺少前端可解析的端点信息。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "证据链" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("当前事实")).not.toBeInTheDocument();
  });

  it("shows a calm error when the subject resource is absent from the snapshot", async () => {
    const user = userEvent.setup();
    renderItem(
      new TimelineEvidenceAdapter(),
      changeItem({ type: "resource", resource_id: "fixture-resource-gamma" }),
    );

    await user.click(screen.getByText("证据链"));

    expect(
      await screen.findByText("无法读取此资源的证据。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "证据链" }),
    ).not.toBeInTheDocument();
  });
});
