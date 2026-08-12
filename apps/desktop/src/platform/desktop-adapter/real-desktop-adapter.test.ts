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

  it("loads relations for a bounded resource set through the dedicated command", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue({ items: [], page_info: { next_cursor: null } });
    const adapter = new RealDesktopAdapter(fake);

    await adapter.getRelationsForResources({
      resource_ids: ["fixture-resource-alpha", "fixture-resource-beta"],
      limit: 400,
    });

    expect(invoke).toHaveBeenCalledWith("query_relations_for_resources", {
      request: {
        resource_ids: ["fixture-resource-alpha", "fixture-resource-beta"],
        limit: 400,
      },
    });
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

  it("sends a GitHub connection request only to the dedicated local command", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue({ connection_id: "github-fixture", sync_run_id: "fixture-run" });
    const adapter = new RealDesktopAdapter(fake);

    await expect(adapter.createGitHubConnection({
      display_name: "Personal GitHub",
      token: "test-token",
      selected_repository_ids: ["fixture-repository"],
    })).resolves.toEqual({ connection_id: "github-fixture", sync_run_id: "fixture-run" });
    expect(invoke).toHaveBeenCalledWith("github_connect", {
      request: {
        display_name: "Personal GitHub",
        token: "test-token",
        selected_repository_ids: ["fixture-repository"],
      },
    });
  });

  it("loads the bounded GitHub repository selection before connection creation", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue([{ id: "fixture-repository", name: "fixture/repository" }]);
    const adapter = new RealDesktopAdapter(fake);

    await expect(adapter.discoverGitHubRepositories("test-token")).resolves.toEqual([
      { id: "fixture-repository", name: "fixture/repository" },
    ]);
    expect(invoke).toHaveBeenCalledWith("github_discover_repositories", {
      request: { token: "test-token" },
    });
  });

  it("previews and purges one connection through dedicated local commands", async () => {
    const { fake, invoke } = transport();
    const summary = {
      resources: 4,
      relations: 3,
      resource_versions: 2,
      relation_versions: 1,
      changes: 5,
      bindings: 0,
      sync_runs: 1,
    };
    invoke.mockResolvedValue(summary);
    const adapter = new RealDesktopAdapter(fake);

    await expect(adapter.previewConnectionPurge("github-fixture")).resolves.toEqual(summary);
    await expect(adapter.purgeConnection("github-fixture")).resolves.toEqual(summary);

    expect(invoke).toHaveBeenNthCalledWith(1, "connection_purge_preview", {
      request: { connection_id: "github-fixture" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "connection_purge", {
      request: { connection_id: "github-fixture" },
    });
  });

  it("maps binding mutations to dedicated local commands", async () => {
    const { fake, invoke } = transport();
    invoke.mockResolvedValue({ binding: {} });
    const adapter = new RealDesktopAdapter(fake);
    const create = {
      source_resource_id: "fixture-source",
      target_resource_id: "fixture-target",
      kind: "infra.depends_on",
    };

    await adapter.createBinding(create);
    await adapter.updateBinding({ binding_id: "fixture-binding", ...create });
    await adapter.disableBinding({ binding_id: "fixture-binding" });

    expect(invoke).toHaveBeenNthCalledWith(1, "binding_create", { request: create });
    expect(invoke).toHaveBeenNthCalledWith(2, "binding_update", {
      request: { binding_id: "fixture-binding", ...create },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "binding_disable", {
      request: { binding_id: "fixture-binding" },
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
