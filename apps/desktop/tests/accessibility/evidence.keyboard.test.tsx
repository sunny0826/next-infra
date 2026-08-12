import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RelationDto } from "../../src/generated/query/RelationDto";
import { EvidenceCard } from "../../src/features/evidence/EvidenceCard";
import { EvidenceSpine } from "../../src/features/evidence/EvidenceSpine";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../src/test/fixtures/query-fixtures";

import { renderDesktopFixture } from "../support/renderDesktopFixture";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const LONG_CONNECTION_ID = "fixture-connection-very-long-identifier-0001";

function providerRelation(connectionId: string): RelationDto {
  return {
    relation_id: "fixture-relation-provider-alpha-beta",
    source_resource_id: "fixture-resource-alpha",
    target_resource_id: "fixture-resource-beta",
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: "fixture",
      connection_id: connectionId,
      sync_run_id: "fixture-sync-run-complete",
      field_path: "attributes.target",
    },
    last_seen_at: "2000-01-01T00:00:00Z",
  };
}

function evidenceSpineFixture() {
  const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
  const source = snapshot.resources.find(
    ({ resource_id }) => resource_id === "fixture-resource-alpha",
  );
  const target = snapshot.resources.find(
    ({ resource_id }) => resource_id === "fixture-resource-beta",
  );
  if (source === undefined || target === undefined) {
    throw new Error("evidence fixture endpoints are missing");
  }
  return { source, target, relations: snapshot.relations };
}

/**
 * Tabs from the current focus until `target` owns focus, bounding the walk so
 * a missed target fails loudly instead of looping forever. A bounded loop is
 * required because jsdom renders every element as visible: elements a real
 * browser would skip (closed <details> content, [hidden] regions) stay in the
 * simulated tab order here.
 */
async function tabTo(
  user: ReturnType<typeof userEvent.setup>,
  target: Element,
  maxTabs = 80,
) {
  for (let step = 0; step < maxTabs && document.activeElement !== target; step += 1) {
    await user.tab();
  }
  expect(target).toHaveFocus();
}

describe("evidence keyboard contract — topology edge and inspector buttons", () => {
  it("opens the relation Inspector from a focused edge with Enter and closes it again", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

    // The fixture adapter only answers topology for a real focus resource, so
    // the bounded focus is established through the search first (same setup
    // as the existing shell tests); the keyboard contract is exercised from
    // there on.
    const search = screen.getByRole("combobox", { name: "搜索本地基础设施" });
    await user.type(search, "alpha");
    await user.click(
      await screen.findByRole("option", { name: /Fixture Compute Alpha/ }),
    );
    await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" });
    await user.click(screen.getByRole("button", { name: "拓扑" }));

    const edge = await screen.findByRole("button", {
      name: "提供方关系 fixture.depends_on",
    });

    await tabTo(user, edge);
    await user.keyboard("{Enter}");

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    await within(inspector).findByRole("heading", { name: "证据路径" });
    expect(within(inspector).getAllByRole("heading", { name: "证据链" })).not.toHaveLength(0);
    expect(within(inspector).getByRole("heading", { name: "当前事实" })).toBeInTheDocument();
    expect(within(inspector).getByLabelText("提供方 evidence")).toBeInTheDocument();

    const close = within(inspector).getByRole("button", { name: "关闭检查器" });
    await tabTo(user, close);
    await user.keyboard("{Enter}");
    expect(inspector).toHaveAttribute("hidden");

    const open = screen.getByRole("button", { name: "打开检查器" });
    await tabTo(user, open);
    await user.keyboard("{Enter}");
    expect(inspector).not.toHaveAttribute("hidden");
  });
});

describe("evidence keyboard contract — spine expand toggle", () => {
  it("expands the evidence list with Enter and collapses it with Space", async () => {
    const user = userEvent.setup();
    const fixture = evidenceSpineFixture();
    const longRelations = [
      ...fixture.relations,
      ...fixture.relations.map((relation) => ({
        ...relation,
        relation_id: `${relation.relation_id}-b`,
      })),
    ];

    render(
      <EvidenceSpine source={fixture.source} target={fixture.target} relations={longRelations} />,
    );

    const toggle = screen.getByRole("button", { name: "展开全部 6 条证据" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    const controlsId = toggle.getAttribute("aria-controls");
    expect(controlsId).not.toBeNull();
    const list = document.getElementById(controlsId ?? "");
    expect(list?.tagName).toBe("OL");
    expect(list?.className).toContain("evidence-spine__path");

    await tabTo(user, toggle);
    await user.keyboard("{Enter}");

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();
    expect(screen.getAllByLabelText(/ evidence$/)).toHaveLength(6);

    await user.keyboard("{ }");

    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.getAllByLabelText(/ evidence$/)).toHaveLength(5);
    expect(screen.queryByText("fixture-relation-inferred-alpha-beta-b")).not.toBeInTheDocument();
  });
});

describe("evidence keyboard contract — copy button", () => {
  it("copies the full identifier with Enter after tabbing to the copy button", async () => {
    // userEvent.setup() first: its clipboard stub replaces navigator.clipboard,
    // so the vi.stubGlobal stub must win the last write.
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });

    render(<EvidenceCard relation={providerRelation(LONG_CONNECTION_ID)} defaultOpen />);

    const copyButton = screen.getByRole("button", { name: `复制 ${LONG_CONNECTION_ID}` });
    await tabTo(user, copyButton);
    expect(copyButton).toHaveFocus();

    await user.keyboard("{Enter}");

    expect(writeText).toHaveBeenCalledWith(LONG_CONNECTION_ID);
    expect(await screen.findByText("已复制")).toBeInTheDocument();
  });
});

describe("evidence keyboard contract — details summary", () => {
  it("expands the details summary with Enter before the copy button is reachable", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });

    render(<EvidenceCard relation={providerRelation(LONG_CONNECTION_ID)} />);

    const summary = screen.getByText(/提供方证据/);
    summary.focus();
    expect(summary).toHaveFocus();

    // jsdom only runs the native <details> toggle for the click event, and
    // user-event never synthesizes that click for <summary> (its Enter/Space
    // default actions cover buttons, inputs, and links only). A browser fires
    // the click as the default action of Enter on the summary, so the keydown
    // is dispatched first and the click stands in for the default action.
    fireEvent.keyDown(summary, { key: "Enter" });
    fireEvent.click(summary);
    expect(summary.closest("details")).toHaveAttribute("open");

    const copyButton = screen.getByRole("button", { name: `复制 ${LONG_CONNECTION_ID}` });
    await tabTo(user, copyButton);
    await user.keyboard("{Enter}");

    expect(writeText).toHaveBeenCalledWith(LONG_CONNECTION_ID);
    expect(await screen.findByText("已复制")).toBeInTheDocument();
  });
});
