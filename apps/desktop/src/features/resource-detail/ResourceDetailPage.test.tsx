import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";
import { ResourceDetailPage } from "./ResourceDetailPage";

afterEach(cleanup);

describe("ResourceDetailPage", () => {
  it("keeps healthy and expired as independent facts", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ResourceDetailPage resourceId="fixture-resource-beta" /></DesktopAdapterProvider>);
    expect(await screen.findByRole("heading", { name: "Fixture Database Beta" })).toBeInTheDocument();
    expect(screen.getAllByText("健康").length).toBeGreaterThan(0);
    expect(screen.getAllByText("已过期").length).toBeGreaterThan(0);
  });

  it("preserves provider configured and inferred evidence", async () => {
    render(<DesktopAdapterProvider adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}><ResourceDetailPage resourceId="fixture-resource-alpha" /></DesktopAdapterProvider>);
    expect(await screen.findAllByText("提供方")).not.toHaveLength(0);
    expect(screen.getAllByText("已配置")).not.toHaveLength(0);
    expect(screen.getAllByText("推断")).not.toHaveLength(0);
  });
});
