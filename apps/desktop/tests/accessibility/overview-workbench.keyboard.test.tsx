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

describe("overview workbench keyboard contract", () => {
  it("activates 查看资源 with Enter and opens the inspector for that resource", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();
    const viewButtons = await screen.findAllByRole("button", { name: "查看资源" });
    expect(viewButtons).toHaveLength(3);

    await tabTo(user, viewButtons[0]);
    await user.keyboard("{Enter}");

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    expect(inspector).not.toHaveAttribute("hidden");
    expect(
      await within(inspector).findByRole("heading", { level: 3, name: "Fixture Database Beta" }),
    ).toBeInTheDocument();
  });

  it("activates 核验证据 with Enter and reflects the disclosure state", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();
    const firstItem = (await screen.findAllByRole("button", { name: "查看资源" }))[0].closest(
      ".overview-attention-item",
    );
    if (firstItem === null) throw new Error("attention item wrapper was not rendered");

    const verify = within(firstItem as HTMLElement).getByRole("button", { name: "核验证据" });
    expect(verify).toHaveAttribute("aria-expanded", "false");

    await tabTo(user, verify);
    await user.keyboard("{Enter}");
    expect(verify).toHaveAttribute("aria-expanded", "true");
    expect(
      within(firstItem as HTMLElement).getByRole("button", { name: "收起证据" }),
    ).toBeInTheDocument();
    expect(
      await within(firstItem as HTMLElement).findByText("Fixture Compute Alpha"),
    ).toBeInTheDocument();

    await user.keyboard("{Enter}");
    expect(verify).toHaveAttribute("aria-expanded", "false");
    expect(within(firstItem as HTMLElement).queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
  });

  it("collapses the first disclosure when a second one opens", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();
    const viewButtons = await screen.findAllByRole("button", { name: "查看资源" });
    const items = viewButtons.map(
      (button) => button.closest(".overview-attention-item") as HTMLElement,
    );

    const firstVerify = within(items[0]).getByRole("button", { name: "核验证据" });
    const secondVerify = within(items[1]).getByRole("button", { name: "核验证据" });

    await tabTo(user, firstVerify);
    await user.keyboard("{Enter}");
    expect(firstVerify).toHaveAttribute("aria-expanded", "true");

    await tabTo(user, secondVerify);
    await user.keyboard("{Enter}");
    expect(secondVerify).toHaveAttribute("aria-expanded", "true");
    expect(within(items[1]).getByRole("button", { name: "收起证据" })).toBeInTheDocument();
    expect(firstVerify).toHaveAttribute("aria-expanded", "false");
    expect(within(items[0]).queryByText("Fixture Compute Alpha")).not.toBeInTheDocument();
  });
});
