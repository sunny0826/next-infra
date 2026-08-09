import { describe, expect, it } from "vitest";

import {
  MANUAL_RELATION_FIXTURE_OBSERVED_AT,
  createManualRelationAdapter,
  createManualRelationSnapshotFixture,
} from "./manual-relation-adapter";

describe("ManualRelationAdapter", () => {
  it("covers every synthetic provider resource required by the manual relation path", async () => {
    const adapter = createManualRelationAdapter();
    const resources = await adapter.searchResources();
    const connections = await adapter.listConnections();

    expect(resources.items.map(({ kind }) => kind)).toEqual([
      "supabase.self_hosted.instance",
      "dokploy.project",
      "dokploy.application",
      "dokploy.domain",
      "tencent.cvm.instance",
      "ssh.host",
      "github.workflow",
      "cloudflare.dns_record",
      "supabase.managed.project",
    ]);
    expect(new Set(connections.items.map(({ connector_type }) => connector_type))).toEqual(
      new Set([
        "supabase-self-hosted",
        "dokploy",
        "tencent",
        "ssh",
        "github",
        "cloudflare",
        "supabase-managed",
      ]),
    );
    expect(resources.items.every(({ observed_at }) => observed_at === MANUAL_RELATION_FIXTURE_OBSERVED_AT)).toBe(true);
  });

  it("creates a cross-connection configured relation that is visible after topology re-query", async () => {
    const adapter = createManualRelationAdapter();
    const result = await adapter.createBinding({
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
    });

    expect(result.binding.status).toBe("active");
    const binding = await adapter.getBinding(result.binding.binding_id);
    expect(binding).toEqual(result.binding);

    const topology = await adapter.getTopology({
      focus_resource_id: "fixture-resource-github-workflow",
      depth: 1,
      max_nodes: 100,
      max_edges: 200,
    });
    expect(topology.edges).toContainEqual(expect.objectContaining({
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
      evidence_type: "configured",
      lifecycle: "active",
      evidence: expect.objectContaining({ binding_id: result.binding.binding_id }),
    }));
  });

  it("makes update and disable authoritative in both binding and topology queries", async () => {
    const adapter = createManualRelationAdapter();
    const created = await adapter.createBinding({
      source_resource_id: "fixture-resource-cloudflare-dns",
      target_resource_id: "fixture-resource-dokploy-domain",
      kind: "network.routes_to",
    });

    const updated = await adapter.updateBinding({
      binding_id: created.binding.binding_id,
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
    });
    expect((await adapter.getBinding(updated.binding.binding_id)).kind).toBe("data.writes_to");

    const afterUpdate = await adapter.getTopology({ focus_resource_id: "fixture-resource-github-workflow" });
    expect(afterUpdate.edges).toContainEqual(expect.objectContaining({
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
      lifecycle: "active",
    }));

    const disabled = await adapter.disableBinding({ binding_id: updated.binding.binding_id });
    expect(disabled.binding.status).toBe("disabled");
    const afterDisable = await adapter.getTopology({ focus_resource_id: "fixture-resource-github-workflow" });
    expect(afterDisable.edges).toContainEqual(expect.objectContaining({
      kind: "data.writes_to",
      lifecycle: "tombstoned",
      evidence_type: "configured",
      evidence: expect.objectContaining({ binding_id: updated.binding.binding_id }),
    }));
  });

  it("rejects duplicate updates and keeps disabled bindings disabled", async () => {
    const adapter = createManualRelationAdapter();
    const created = await adapter.createBinding({
      source_resource_id: "fixture-resource-cloudflare-dns",
      target_resource_id: "fixture-resource-dokploy-domain",
      kind: "network.routes_to",
    });

    await expect(adapter.updateBinding({
      binding_id: created.binding.binding_id,
      source_resource_id: "fixture-resource-supabase-self-hosted-instance",
      target_resource_id: "fixture-resource-dokploy-application",
      kind: "infra.deployed_via",
    })).rejects.toMatchObject({ code: "binding_conflict" });
    await expect(adapter.getBinding(created.binding.binding_id)).resolves.toMatchObject({
      kind: "network.routes_to",
      status: "active",
    });

    await adapter.disableBinding({ binding_id: created.binding.binding_id });
    const updated = await adapter.updateBinding({
      binding_id: created.binding.binding_id,
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
    });
    expect(updated.binding.status).toBe("disabled");
  });

  it("keeps an unresolved configured relation and its missing endpoint visible", async () => {
    const adapter = createManualRelationAdapter();
    const topology = await adapter.getTopology({ focus_resource_id: "fixture-resource-dokploy-application" });
    const unresolved = topology.edges.find((relation) =>
      relation.evidence.type === "configured" && relation.lifecycle === "orphaned",
    );

    expect(unresolved).toBeDefined();
    expect(unresolved?.target_resource_id).toBe("fixture-resource-missing-host");
    expect(topology.nodes.some(({ resource_id }) => resource_id === unresolved?.target_resource_id)).toBe(false);
    expect(unresolved?.evidence).toEqual(expect.objectContaining({ binding_id: "fixture-binding-missing-host" }));
    await expect(adapter.getBinding("fixture-binding-missing-host")).resolves.toEqual(expect.objectContaining({ status: "unresolved" }));
  });

  it("contains only deterministic synthetic values", () => {
    const serialized = JSON.stringify(createManualRelationSnapshotFixture());
    expect(serialized).toContain("fixture-");
    expect(serialized).not.toMatch(/github\.com|10\.0\.|192\.168\.|https?:\/\//i);
    expect(serialized).not.toMatch(/secret|password|token/i);
    expect(serialized).toContain(MANUAL_RELATION_FIXTURE_OBSERVED_AT);
  });
});
