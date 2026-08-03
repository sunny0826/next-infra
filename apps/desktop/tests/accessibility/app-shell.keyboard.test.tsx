import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { renderDesktopFixture } from "../support/renderDesktopFixture";

afterEach(cleanup);

describe("Goal 3 shell keyboard contract", () => {
  it("focuses bounded search and clears its listbox with Escape", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const search = screen.getByRole("combobox", {
      name: "Search local infrastructure",
    });
    expect(search).toHaveFocus();

    await user.type(search, "alpha");
    expect(await screen.findByRole("listbox", { name: "Search results" })).toBeInTheDocument();

    fireEvent.keyDown(search, { key: "Escape" });
    await waitFor(() => {
      expect(search).toHaveValue("");
      expect(search).not.toHaveFocus();
      expect(screen.queryByRole("listbox", { name: "Search results" })).not.toBeInTheDocument();
    });
  });

  it("opens Inventory detail from a focused row with Space", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

    await user.click(screen.getByRole("button", { name: "Inventory" }));
    const row = (await screen.findByText("Fixture Compute Alpha")).closest("tr");
    expect(row).toHaveAttribute("tabindex", "0");

    row?.focus();
    fireEvent.keyDown(row!, { key: " " });

    expect(
      await screen.findByRole("heading", { level: 1, name: "Fixture Compute Alpha" }),
    ).toBeInTheDocument();
    const inspector = screen.getByRole("complementary", { name: "Evidence inspector" });
    expect(
      within(inspector).getByRole("heading", { level: 3, name: "Fixture Compute Alpha" }),
    ).toBeInTheDocument();
  });
});
