import { describe, expect, it } from "vitest";

import { EmptyDesktopAdapter } from "./empty-desktop-adapter";

describe("EmptyDesktopAdapter", () => {
  it("reports the unavailable query service instead of a fake empty snapshot", async () => {
    const adapter = new EmptyDesktopAdapter();
    await expect(adapter.listConnections()).rejects.toThrow(
      "Desktop query service is unavailable.",
    );
  });
});
