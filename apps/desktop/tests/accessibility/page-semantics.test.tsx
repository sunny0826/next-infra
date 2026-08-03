import { cleanup, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { renderDesktopFixture } from "../support/renderDesktopFixture";

afterEach(cleanup);

describe("Goal 3 shell semantics", () => {
  it("keeps navigation, search, main, inspector, and runtime landmarks named", async () => {
    renderDesktopFixture();

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    expect(within(navigation).getByRole("button", { name: "Overview" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("combobox", { name: "Search local infrastructure" })).toHaveAttribute(
      "aria-keyshortcuts",
      "Meta+K Control+K",
    );
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Evidence inspector" })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo", { name: "Control Plane Runtime" })).toBeInTheDocument();
    expect(
      await screen.findByRole("button", {
        name: /Fixture Database Beta.*Freshness expired/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("unreachable")).toBeInTheDocument();
    expect(screen.getByText("disabled")).toBeInTheDocument();
  });

  it("labels Timeline as unavailable instead of an empty result", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

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
});
