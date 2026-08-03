import { describe, expect, it, vi } from "vitest";

import type { DesktopTransport } from "./real-desktop-adapter";
import {
  DesktopAdapterError,
  RealDesktopAdapter,
} from "./real-desktop-adapter";

function transport() {
  const invoke = vi.fn();
  const listen = vi.fn().mockResolvedValue(() => undefined);
  return {
    fake: {
      invoke: invoke as unknown as DesktopTransport["invoke"],
      listen: listen as unknown as DesktopTransport["listen"],
    },
    invoke,
    listen,
  };
}

describe("RealDesktopAdapter", () => {
  it("maps bounded query requests to stable command names", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue({ items: [] });
    const adapter = new RealDesktopAdapter(fake);

    await adapter.searchResources({ limit: 25, query: "fixture" });
    await adapter.getTopology({
      focus_resource_id: "fixture-resource",
      depth: 1,
      max_nodes: 100,
      max_edges: 200,
    });
    await adapter.listConnections();

    expect(invoke).toHaveBeenNthCalledWith(1, "query_search_resources", {
      request: { limit: 25, query: "fixture" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "query_get_topology", {
      request: {
        focus_resource_id: "fixture-resource",
        depth: 1,
        max_nodes: 100,
        max_edges: 200,
      },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "query_list_connections", undefined);
  });

  it("returns the manual sync run id without treating it as a UI refresh", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue({ sync_run_id: "fixture-run" });
    const adapter = new RealDesktopAdapter(fake);

    await expect(adapter.manualSync("fixture-connection")).resolves.toEqual({
      sync_run_id: "fixture-run",
    });
    expect(invoke).toHaveBeenCalledWith("runtime_manual_sync", {
      connectionId: "fixture-connection",
    });
  });

  it("cleans unknown transport failures and preserves safe envelopes", async () => {
    const { fake, invoke } = transport();
    const adapter = new RealDesktopAdapter(fake);
    invoke.mockRejectedValueOnce(new Error("secret-bearing platform error"));

    await expect(adapter.getHealthSummary()).rejects.toEqual(
      new DesktopAdapterError(
        "desktop_transport_failed",
        "The local desktop service could not complete the request.",
        true,
      ),
    );

    invoke.mockRejectedValueOnce({
      schema_version: 1,
      code: "query_unavailable",
      message: "Query service is unavailable.",
      retryable: true,
    });
    await expect(adapter.getHealthSummary()).rejects.toEqual(
      new DesktopAdapterError(
        "query_unavailable",
        "Query service is unavailable.",
        true,
      ),
    );
  });

  it("subscribes only to invalidation metadata", async () => {
    const { fake, listen } = transport();
    const adapter = new RealDesktopAdapter(fake);
    const listener = vi.fn();

    await adapter.subscribeInvalidations(listener);

    expect(listen).toHaveBeenCalledWith(
      "next-infra://query-invalidated",
      listener,
    );
  });
});
