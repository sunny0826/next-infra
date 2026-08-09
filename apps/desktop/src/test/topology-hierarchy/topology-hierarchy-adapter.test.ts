import { describe, expect, it } from "vitest";

import {
  createTopologyHierarchyAdapter,
  createTopologyHierarchySnapshotFixture,
  TOPOLOGY_HIERARCHY_FIXTURE_IDS,
} from "./topology-hierarchy-adapter";
import { TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT } from "./topology-hierarchy-fixture";

const {
  githubRepository,
  githubWorkflow,
  githubWorkflowRun,
  cloudflareAccount,
  cloudflareZone,
  cloudflareTunnel,
  cloudflareWorker,
  sshHost,
  sshFilesystem,
  sshProcess,
  sshService,
  missingService,
} = TOPOLOGY_HIERARCHY_FIXTURE_IDS;

describe("TopologyHierarchyAdapter", () => {
  it("contains the current provider hierarchy scenarios without removed GitHub kinds", () => {
    const snapshot = createTopologyHierarchySnapshotFixture();
    const kinds = snapshot.resources.map(({ kind }) => kind);

    expect(kinds).toEqual(expect.arrayContaining([
      "github.repository",
      "github.workflow",
      "github.workflow_run",
      "cloudflare.account",
      "cloudflare.zone",
      "cloudflare.tunnel",
      "cloudflare.worker",
      "ssh.host",
      "ssh.filesystem",
      "ssh.process-summary",
      "ssh.launchd-service",
    ]));
    expect(kinds).not.toEqual(expect.arrayContaining([
      "github.environment",
      "github.deployment",
      "github.job",
      "github.pages",
    ]));
    expect(snapshot.resources.every(({ observed_at }) => observed_at === TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT)).toBe(true);
  });

  it("returns only direct repository adjacency, excluding workflow runs", async () => {
    const topology = await createTopologyHierarchyAdapter().getTopology({
      focus_resource_id: githubRepository,
      depth: 1,
    });

    expect(topology.nodes.map(({ resource_id }) => resource_id)).toEqual([
      githubRepository,
      githubWorkflow,
    ]);
    expect(topology.edges.map(({ kind }) => kind)).toEqual(["github.contains"]);
    expect(topology.nodes.some(({ resource_id }) => resource_id === githubWorkflowRun)).toBe(false);
    expect(topology.edges.every(({ source_resource_id, target_resource_id }) =>
      source_resource_id === githubRepository || target_resource_id === githubRepository,
    )).toBe(true);
  });

  it("returns the workflow parent and its direct run, excluding unrelated providers", async () => {
    const topology = await createTopologyHierarchyAdapter().getTopology({
      focus_resource_id: githubWorkflow,
      depth: 1,
    });

    expect(topology.nodes.map(({ resource_id }) => resource_id)).toEqual([
      githubWorkflow,
      githubRepository,
      githubWorkflowRun,
    ]);
    expect(topology.edges.map(({ kind }) => kind)).toEqual([
      "github.contains",
      "github.executes",
    ]);
    expect(topology.nodes.some(({ resource_id }) =>
      [cloudflareAccount, cloudflareZone, sshHost].includes(resource_id as typeof cloudflareAccount),
    )).toBe(false);
  });

  it("returns direct Cloudflare account children and direct SSH host children", async () => {
    const adapter = createTopologyHierarchyAdapter();
    const cloudflare = await adapter.getTopology({ focus_resource_id: cloudflareAccount });
    const ssh = await adapter.getTopology({ focus_resource_id: sshHost });

    expect(cloudflare.nodes.map(({ resource_id }) => resource_id)).toEqual([
      cloudflareAccount,
      cloudflareZone,
      cloudflareTunnel,
      cloudflareWorker,
    ]);
    expect(cloudflare.edges).toHaveLength(3);
    expect(cloudflare.edges.every(({ kind }) => kind === "cloudflare.contains")).toBe(true);

    expect(ssh.nodes.map(({ resource_id }) => resource_id)).toEqual([
      sshHost,
      sshFilesystem,
      sshProcess,
      sshService,
    ]);
    expect(ssh.edges).toHaveLength(3);
    expect(ssh.edges.every(({ kind }) => kind === "ssh.contains")).toBe(true);
  });

  it("preserves configured cross-provider and orphaned relations", async () => {
    const snapshot = createTopologyHierarchySnapshotFixture();
    const configured = snapshot.relations.filter(({ evidence_type }) => evidence_type === "configured");
    const crossProvider = configured.find(({ relation_id }) => relation_id.includes("worker-service"));
    const orphaned = configured.find(({ relation_id }) => relation_id.includes("orphaned"));

    expect(configured).toHaveLength(2);
    expect(crossProvider).toEqual(expect.objectContaining({
      kind: "infra.depends_on",
      lifecycle: "active",
      source_resource_id: cloudflareWorker,
      target_resource_id: sshService,
      evidence_type: "configured",
    }));
    expect(orphaned).toEqual(expect.objectContaining({
      kind: "infra.depends_on",
      lifecycle: "orphaned",
      source_resource_id: cloudflareTunnel,
      target_resource_id: missingService,
      evidence_type: "configured",
    }));

    const orphanTopology = await createTopologyHierarchyAdapter().getTopology({
      focus_resource_id: cloudflareTunnel,
    });
    expect(orphanTopology.nodes.map(({ resource_id }) => resource_id)).toEqual([
      cloudflareTunnel,
      cloudflareAccount,
    ]);
    expect(orphanTopology.edges).toContainEqual(orphaned);
    expect(orphanTopology.nodes.some(({ resource_id }) => resource_id === missingService)).toBe(false);
  });

  it("enforces node and edge bounds and reports omitted direct neighbors as frontier", async () => {
    const adapter = createTopologyHierarchyAdapter();
    const byNodes = await adapter.getTopology({
      focus_resource_id: cloudflareAccount,
      depth: 1,
      max_nodes: 2,
      max_edges: 200,
    });
    const byEdges = await adapter.getTopology({
      focus_resource_id: cloudflareAccount,
      depth: 1,
      max_nodes: 100,
      max_edges: 1,
    });

    for (const topology of [byNodes, byEdges]) {
      expect(topology.nodes.length).toBeLessThanOrEqual(2);
      expect(topology.edges.length).toBeLessThanOrEqual(1);
      expect(topology.truncated).toBe(true);
      expect(topology.frontier).toEqual([
        { resource_id: cloudflareTunnel, direction: "outgoing" },
        { resource_id: cloudflareWorker, direction: "outgoing" },
      ]);
    }
    expect(byNodes.nodes.map(({ resource_id }) => resource_id)).toEqual([
      cloudflareAccount,
      cloudflareZone,
    ]);
    expect(byEdges.edges.map(({ target_resource_id }) => target_resource_id)).toEqual([cloudflareZone]);
  });

  it("rejects unsupported depth and invalid bounds", async () => {
    const adapter = createTopologyHierarchyAdapter();

    await expect(adapter.getTopology({ focus_resource_id: githubRepository, depth: 2 })).rejects.toThrow(/depth 1/);
    await expect(adapter.getTopology({ focus_resource_id: githubRepository, max_nodes: 0 })).rejects.toThrow(/max_nodes/);
    await expect(adapter.getTopology({ focus_resource_id: githubRepository, max_edges: 401 })).rejects.toThrow(/max_edges/);
  });

  it("keeps fixture values synthetic and deterministic", () => {
    const first = JSON.stringify(createTopologyHierarchySnapshotFixture());
    const second = JSON.stringify(createTopologyHierarchySnapshotFixture());

    expect(first).toBe(second);
    expect(first).toContain("fixture-");
    expect(first).not.toMatch(/github\.com|10\.0\.|192\.168\.|https?:\/\//i);
    expect(first).not.toMatch(/secret|password|token/i);
  });
});
