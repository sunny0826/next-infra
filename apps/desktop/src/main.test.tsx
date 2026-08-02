import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { App } from "./main";

describe("desktop bootstrap host", () => {
  it("renders the empty Next Infra host", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "Next Infra" })).toBeInTheDocument();
  });
});
