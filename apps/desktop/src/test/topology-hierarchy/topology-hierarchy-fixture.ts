import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type { DesktopAdapterSnapshot } from "../../platform/desktop-adapter/mock-desktop-adapter";

export const TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT = "2000-01-01T00:00:00Z";

export const TOPOLOGY_HIERARCHY_FIXTURE_IDS = {
  githubRepository: "fixture-topology-github-repository",
  githubWorkflow: "fixture-topology-github-workflow",
  githubWorkflowRun: "fixture-topology-github-workflow-run",
  cloudflareAccount: "fixture-topology-cloudflare-account",
  cloudflareZone: "fixture-topology-cloudflare-zone",
  cloudflareTunnel: "fixture-topology-cloudflare-tunnel",
  cloudflareWorker: "fixture-topology-cloudflare-worker",
  sshHost: "fixture-topology-ssh-host",
  sshFilesystem: "fixture-topology-ssh-filesystem",
  sshProcess: "fixture-topology-ssh-process",
  sshService: "fixture-topology-ssh-service",
  missingService: "fixture-topology-missing-service",
} as const;

const connectionIds = {
  github: "fixture-topology-github-connection",
  cloudflare: "fixture-topology-cloudflare-connection",
  ssh: "fixture-topology-ssh-connection",
} as const;

function metadata(): SnapshotMetadata {
  return {
    schema_version: 1,
    snapshot_version: "fixture-topology-hierarchy-v1",
    generated_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
  };
}

function resource(
  resourceId: string,
  connectionId: string,
  kind: string,
  displayName: string,
): ResourceDto {
  return {
    resource_id: resourceId,
    connection_id: connectionId,
    kind,
    display_name: displayName,
    scope: "fixture-topology-scope",
    lifecycle: "active",
    health: "healthy",
    freshness: "fresh",
    observed_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
  };
}

function connection(
  connectionId: string,
  connectorType: string,
  displayName: string,
): ConnectionDto {
  return {
    connection_id: connectionId,
    connector_type: connectorType,
    display_name: displayName,
    enabled: true,
    health: "healthy",
    last_success_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
    last_attempt_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
  };
}

function providerRelation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind: string,
  connectionId: string,
  connectorType: string,
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
    lifecycle: "active",
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: connectorType,
      connection_id: connectionId,
      sync_run_id: `fixture-topology-${connectorType}-sync-run`,
      field_path: "attributes.fixture_parent",
    },
    last_seen_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
  };
}

function configuredRelation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind: string,
  bindingId: string,
  lifecycle: RelationDto["lifecycle"] = "active",
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
    lifecycle,
    evidence_type: "configured",
    evidence: {
      type: "configured",
      binding_id: bindingId,
      created_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
    },
    last_seen_at: TOPOLOGY_HIERARCHY_FIXTURE_OBSERVED_AT,
  };
}

/**
 * A deterministic, provider-shaped topology snapshot used by bounded topology
 * acceptance tests. The roots intentionally have only direct provider edges;
 * configured edges connect child resources so root hierarchy assertions remain
 * representative of one provider's observed containment.
 */
export function createTopologyHierarchySnapshotFixture(): DesktopAdapterSnapshot {
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

  return {
    metadata: metadata(),
    resources: [
      resource(githubRepository, connectionIds.github, "github.repository", "Fixture GitHub Repository"),
      resource(githubWorkflow, connectionIds.github, "github.workflow", "Fixture GitHub Workflow"),
      resource(githubWorkflowRun, connectionIds.github, "github.workflow_run", "Fixture GitHub Workflow Run"),
      resource(cloudflareAccount, connectionIds.cloudflare, "cloudflare.account", "Fixture Cloudflare Account"),
      resource(cloudflareZone, connectionIds.cloudflare, "cloudflare.zone", "Fixture Cloudflare Zone"),
      resource(cloudflareTunnel, connectionIds.cloudflare, "cloudflare.tunnel", "Fixture Cloudflare Tunnel"),
      resource(cloudflareWorker, connectionIds.cloudflare, "cloudflare.worker", "Fixture Cloudflare Worker"),
      resource(sshHost, connectionIds.ssh, "ssh.host", "Fixture SSH Host"),
      resource(sshFilesystem, connectionIds.ssh, "ssh.filesystem", "Fixture SSH Filesystem"),
      resource(sshProcess, connectionIds.ssh, "ssh.process-summary", "Fixture SSH Process Summary"),
      resource(sshService, connectionIds.ssh, "ssh.launchd-service", "Fixture SSH Service"),
    ],
    relations: [
      providerRelation(
        "fixture-topology-relation-github-01-repository-workflow",
        githubRepository,
        githubWorkflow,
        "github.contains",
        connectionIds.github,
        "github",
      ),
      providerRelation(
        "fixture-topology-relation-github-02-workflow-run",
        githubWorkflow,
        githubWorkflowRun,
        "github.executes",
        connectionIds.github,
        "github",
      ),
      providerRelation(
        "fixture-topology-relation-cloudflare-01-account-zone",
        cloudflareAccount,
        cloudflareZone,
        "cloudflare.contains",
        connectionIds.cloudflare,
        "cloudflare",
      ),
      providerRelation(
        "fixture-topology-relation-cloudflare-02-account-tunnel",
        cloudflareAccount,
        cloudflareTunnel,
        "cloudflare.contains",
        connectionIds.cloudflare,
        "cloudflare",
      ),
      providerRelation(
        "fixture-topology-relation-cloudflare-03-account-worker",
        cloudflareAccount,
        cloudflareWorker,
        "cloudflare.contains",
        connectionIds.cloudflare,
        "cloudflare",
      ),
      providerRelation(
        "fixture-topology-relation-ssh-01-host-filesystem",
        sshHost,
        sshFilesystem,
        "ssh.contains",
        connectionIds.ssh,
        "ssh",
      ),
      providerRelation(
        "fixture-topology-relation-ssh-02-host-process",
        sshHost,
        sshProcess,
        "ssh.contains",
        connectionIds.ssh,
        "ssh",
      ),
      providerRelation(
        "fixture-topology-relation-ssh-03-host-service",
        sshHost,
        sshService,
        "ssh.contains",
        connectionIds.ssh,
        "ssh",
      ),
      configuredRelation(
        "fixture-topology-relation-configured-worker-service",
        cloudflareWorker,
        sshService,
        "infra.depends_on",
        "fixture-topology-binding-worker-service",
      ),
      configuredRelation(
        "fixture-topology-relation-orphaned-tunnel-service",
        cloudflareTunnel,
        missingService,
        "infra.depends_on",
        "fixture-topology-binding-missing-service",
        "orphaned",
      ),
    ],
    connections: [
      connection(connectionIds.github, "github", "Fixture GitHub Connection"),
      connection(connectionIds.cloudflare, "cloudflare", "Fixture Cloudflare Connection"),
      connection(connectionIds.ssh, "ssh", "Fixture SSH Connection"),
    ],
  };
}
