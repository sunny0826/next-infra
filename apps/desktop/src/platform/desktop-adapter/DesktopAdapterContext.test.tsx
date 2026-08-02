import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EmptyDesktopAdapter } from "./empty-desktop-adapter";
import {
  DesktopAdapterProvider,
  useDesktopAdapter,
} from "./DesktopAdapterContext";
import type { DesktopAdapter } from "./desktop-adapter";

function AdapterProbe({ expected }: { readonly expected: DesktopAdapter }) {
  const adapter = useDesktopAdapter();
  return <output>{adapter === expected ? "injected" : "unexpected"}</output>;
}

describe("DesktopAdapterProvider", () => {
  it("injects the selected adapter", () => {
    const adapter = new EmptyDesktopAdapter();

    render(
      <DesktopAdapterProvider adapter={adapter}>
        <AdapterProbe expected={adapter} />
      </DesktopAdapterProvider>,
    );

    expect(screen.getByText("injected")).toBeInTheDocument();
  });

  it("fails clearly when the provider is missing", () => {
    const adapter = new EmptyDesktopAdapter();

    expect(() => render(<AdapterProbe expected={adapter} />)).toThrow(
      "useDesktopAdapter must be used within a DesktopAdapterProvider",
    );
  });
});
