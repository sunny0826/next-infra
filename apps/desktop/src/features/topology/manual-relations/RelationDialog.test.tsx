import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DesktopAdapterProvider } from "../../../platform/desktop-adapter/DesktopAdapterContext";
import { createManualRelationAdapter } from "../../../test/fixtures/manual-relation-adapter";

import { RelationDialog } from "./RelationDialog";

afterEach(cleanup);

function DialogHarness() {
  const [open, setOpen] = useState(false);
  return (
    <DesktopAdapterProvider adapter={createManualRelationAdapter()}>
      <button onClick={() => setOpen(true)} type="button">新增关联</button>
      {open ? (
        <RelationDialog
          onClose={() => setOpen(false)}
          onSaved={vi.fn()}
          relation={null}
          source={null}
        />
      ) : null}
    </DesktopAdapterProvider>
  );
}

describe("RelationDialog", () => {
  it("renders outside the evidence inspector and restores focus after Escape", async () => {
    const user = userEvent.setup();
    render(<DialogHarness />);
    const trigger = screen.getByRole("button", { name: "新增关联" });

    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭关系配置" })).toHaveFocus();
    expect(dialog.closest(".shell-inspector")).toBeNull();

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
  });

  it("closes only when the modal backdrop itself is pressed", async () => {
    const user = userEvent.setup();
    render(<DialogHarness />);
    await user.click(screen.getByRole("button", { name: "新增关联" }));

    const dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    fireEvent.mouseDown(dialog);
    expect(dialog).toBeInTheDocument();

    const overlay = document.querySelector<HTMLElement>(".relation-dialog-overlay");
    if (overlay === null) throw new Error("relation dialog overlay missing");
    fireEvent.mouseDown(overlay);
    expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
  });
});
