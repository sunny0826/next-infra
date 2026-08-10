import { cleanup, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { renderDesktopFixture } from "../support/renderDesktopFixture";

afterEach(cleanup);

describe("Goal 3 shell semantics", () => {
  it("keeps navigation, search, main, inspector, and runtime landmarks named", async () => {
    renderDesktopFixture();

    const navigation = screen.getByRole("navigation", { name: "Primary navigation" });
    expect(within(navigation).getByRole("button", { name: "概览" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("combobox", { name: "搜索本地基础设施" })).toHaveAttribute(
      "aria-keyshortcuts",
      "Meta+K Control+K",
    );
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "证据检查器" })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo", { name: "控制平面运行时" })).toBeInTheDocument();
    expect(
      await screen.findByRole("button", {
        name: /Fixture Database Beta.*已过期/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 异常")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 2, name: "需要关注" }),
    ).toBeInTheDocument();
  });

  it("labels Timeline as a structured committed-change result", async () => {
    const user = userEvent.setup();
    renderDesktopFixture();

    await user.click(screen.getByRole("button", { name: "时间线" }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "时间线" }),
    ).toBeInTheDocument();
    expect(screen.getByText("没有已持久化的变更。")).toBeInTheDocument();
  });
});
