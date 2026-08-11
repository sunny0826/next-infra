import { cleanup, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { renderDesktopFixture } from "../support/renderDesktopFixture";

afterEach(cleanup);

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

/** The two fixed row actions; every other button inside an item is the relation summary. */
const ROW_ACTIONS = new Set(["查看资源", "核验证据", "收起证据"]);

function relationSummaryButton(item: HTMLElement): HTMLElement {
  const candidates = within(item)
    .getAllByRole("button")
    .filter((button) => !ROW_ACTIONS.has(button.textContent?.trim() ?? ""));
  expect(candidates).toHaveLength(1);
  return candidates[0];
}

describe("overview workbench → inspector keyboard path", () => {
  it("opens the existing inspector from a relation summary and returns focus after closing", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

    const firstItem = (await screen.findAllByRole("button", { name: "查看资源" }))[0].closest(
      ".overview-attention-item",
    );
    if (firstItem === null) throw new Error("attention item wrapper was not rendered");
    const item = firstItem as HTMLElement;

    // Expand the evidence disclosure for the first attention item.
    const verify = within(item).getByRole("button", { name: "核验证据" });
    await tabTo(user, verify);
    await user.keyboard("{Enter}");
    expect(verify).toHaveAttribute("aria-expanded", "true");

    // The compact relation summary is a real button; Enter activates it.
    const summary = relationSummaryButton(item);
    await tabTo(user, summary);
    await user.keyboard("{Enter}");

    // The existing Inspector opens with the endpoints and the full aggregated evidence.
    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    expect(inspector).not.toHaveAttribute("hidden");
    expect(await within(inspector).findByText("当前事实")).toBeInTheDocument();
    expect(within(inspector).getByText("Fixture Compute Alpha")).toBeInTheDocument();
    expect(within(inspector).getByText("Fixture Database Beta")).toBeInTheDocument();
    expect(within(inspector).getAllByLabelText(/ evidence$/)).toHaveLength(3);

    // Closing returns focus to the inspector toggle, per the AppShell contract.
    const close = within(inspector).getByRole("button", { name: "关闭检查器" });
    await tabTo(user, close);
    await user.keyboard("{Enter}");
    expect(inspector).toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "打开检查器" })).toHaveFocus();
  });
});
