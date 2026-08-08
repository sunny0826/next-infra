import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { useTheme } from "./useTheme";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

function ThemeProbe() {
  const { theme, toggleTheme } = useTheme();
  return (
    <button onClick={toggleTheme} type="button">
      当前主题:{theme}
    </button>
  );
}

describe("useTheme", () => {
  it("defaults to the dark theme and applies it to the document", () => {
    render(<ThemeProbe />);

    expect(screen.getByRole("button")).toHaveTextContent("当前主题:dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("theme")).toBe("dark");
  });

  it("restores a persisted light preference", () => {
    window.localStorage.setItem("theme", "light");

    render(<ThemeProbe />);

    expect(screen.getByRole("button")).toHaveTextContent("当前主题:light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("toggles between dark and light and persists each choice", async () => {
    const user = userEvent.setup();
    render(<ThemeProbe />);

    await user.click(screen.getByRole("button"));
    expect(screen.getByRole("button")).toHaveTextContent("当前主题:light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(window.localStorage.getItem("theme")).toBe("light");

    await user.click(screen.getByRole("button"));
    expect(screen.getByRole("button")).toHaveTextContent("当前主题:dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("theme")).toBe("dark");
  });
});
