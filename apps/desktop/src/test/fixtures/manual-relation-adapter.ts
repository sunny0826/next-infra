import type { BindingCommandResultDto } from "../../generated/query/BindingCommandResultDto";
import type { BindingDto } from "../../generated/query/BindingDto";
import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type {
  CreateBindingInput,
  DisableBindingInput,
  GetResourceInput,
  GetTopologyInput,
  SearchResourcesInput,
  UpdateBindingInput,
} from "../../platform/desktop-adapter/desktop-adapter";
import {
  MockDesktopAdapter,
  type DesktopAdapterSnapshot,
} from "../../platform/desktop-adapter/mock-desktop-adapter";

export const MANUAL_RELATION_FIXTURE_OBSERVED_AT = "2000-01-01T00:00:00Z";

const UPDATED_AT = "2000-01-01T00:00:01Z";
const MISSING_RESOURCE_ID = "fixture-resource-missing-host";

function metadata(): SnapshotMetadata {
  return {
    schema_version: 1,
    snapshot_version: "fixture-manual-relations-v1",
    generated_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
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
    scope: "fixture-scope",
    lifecycle: "active",
    health: "healthy",
    freshness: "fresh",
    observed_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
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
    last_success_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
    last_attempt_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
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
      sync_run_id: "fixture-manual-relations-sync",
      field_path: "attributes.fixture_relation",
    },
    last_seen_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
  };
}

function inferredRelation(
  relationId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind: string,
): RelationDto {
  return {
    relation_id: relationId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
    lifecycle: "active",
    evidence_type: "inferred",
    evidence: {
      type: "inferred",
      rule_version: "fixture-manual-relations-rule-v1",
      input_resource_version_ids: ["fixture-manual-resource-version-v1"],
      input_relation_version_ids: [],
      confidence_basis_points: 8500,
    },
    last_seen_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
  };
}

function configuredBinding(
  bindingId: string,
  sourceResourceId: string,
  targetResourceId: string,
  kind: string,
  status: BindingDto["status"],
): BindingDto {
  return {
    binding_id: bindingId,
    source_resource_id: sourceResourceId,
    target_resource_id: targetResourceId,
    kind,
    status,
    created_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
    updated_at: MANUAL_RELATION_FIXTURE_OBSERVED_AT,
  };
}

export function createManualRelationSnapshotFixture(): DesktopAdapterSnapshot {
  const connections = [
    connection(
      "fixture-connection-supabase-self-hosted",
      "supabase-self-hosted",
      "Fixture Supabase Self-hosted",
    ),
    connection("fixture-connection-dokploy", "dokploy", "Fixture Dokploy"),
    connection("fixture-connection-tencent", "tencent", "Fixture Tencent"),
    connection("fixture-connection-ssh", "ssh", "Fixture SSH"),
    connection("fixture-connection-github", "github", "Fixture GitHub"),
    connection("fixture-connection-cloudflare", "cloudflare", "Fixture Cloudflare"),
    connection(
      "fixture-connection-supabase-managed",
      "supabase-managed",
      "Fixture Supabase Managed",
    ),
  ];

  const resources = [
    resource(
      "fixture-resource-supabase-self-hosted-instance",
      "fixture-connection-supabase-self-hosted",
      "supabase.self_hosted.instance",
      "Fixture Supabase Self-hosted Instance",
    ),
    resource(
      "fixture-resource-dokploy-project",
      "fixture-connection-dokploy",
      "dokploy.project",
      "Fixture Dokploy Project",
    ),
    resource(
      "fixture-resource-dokploy-application",
      "fixture-connection-dokploy",
      "dokploy.application",
      "Fixture Dokploy Application",
    ),
    resource(
      "fixture-resource-dokploy-domain",
      "fixture-connection-dokploy",
      "dokploy.domain",
      "Fixture Dokploy Domain",
    ),
    resource(
      "fixture-resource-tencent-cvm",
      "fixture-connection-tencent",
      "tencent.cvm.instance",
      "Fixture Tencent CVM",
    ),
    resource(
      "fixture-resource-ssh-host",
      "fixture-connection-ssh",
      "ssh.host",
      "Fixture SSH Host",
    ),
    resource(
      "fixture-resource-github-workflow",
      "fixture-connection-github",
      "github.workflow",
      "Fixture GitHub Workflow",
    ),
    resource(
      "fixture-resource-cloudflare-dns",
      "fixture-connection-cloudflare",
      "cloudflare.dns_record",
      "Fixture Cloudflare DNS",
    ),
    resource(
      "fixture-resource-supabase-managed-project",
      "fixture-connection-supabase-managed",
      "supabase.managed.project",
      "Fixture Supabase Managed Project",
    ),
  ];

  const activeBinding = configuredBinding(
    "fixture-binding-supabase-dokploy",
    "fixture-resource-supabase-self-hosted-instance",
    "fixture-resource-dokploy-application",
    "infra.deployed_via",
    "active",
  );
  const unresolvedBinding = configuredBinding(
    "fixture-binding-missing-host",
    "fixture-resource-dokploy-application",
    MISSING_RESOURCE_ID,
    "infra.accessed_via",
    "unresolved",
  );

  return {
    metadata: metadata(),
    resources,
    relations: [
      providerRelation(
        "fixture-relation-github-dokploy",
        "fixture-resource-github-workflow",
        "fixture-resource-dokploy-application",
        "automation.deploys_to",
        "fixture-connection-github",
        "github",
      ),
      inferredRelation(
        "fixture-relation-tencent-ssh",
        "fixture-resource-tencent-cvm",
        "fixture-resource-ssh-host",
        "infra.accessed_via",
      ),
      relationFromBinding(activeBinding),
      relationFromBinding(unresolvedBinding),
    ],
    connections,
  };
}

function relationFromBinding(binding: BindingDto): RelationDto {
  const unresolved = binding.status === "unresolved";
  return {
    relation_id: `fixture-relation-${binding.binding_id}`,
    source_resource_id: binding.source_resource_id,
    target_resource_id: binding.target_resource_id,
    kind: binding.kind,
    lifecycle: unresolved ? "orphaned" : binding.status === "disabled" ? "tombstoned" : "active",
    evidence_type: "configured",
    evidence: {
      type: "configured",
      binding_id: binding.binding_id,
      created_at: binding.created_at,
    },
    last_seen_at: binding.updated_at,
  };
}

function copyBinding(binding: BindingDto): BindingDto {
  return { ...binding };
}

function copyRelation(relation: RelationDto): RelationDto {
  return {
    ...relation,
    evidence: relation.evidence.type === "provider"
      ? { ...relation.evidence }
      : relation.evidence.type === "configured"
        ? { ...relation.evidence }
        : { ...relation.evidence, input_resource_version_ids: [...relation.evidence.input_resource_version_ids], input_relation_version_ids: [...relation.evidence.input_relation_version_ids] },
  };
}

export class ManualRelationAdapter extends MockDesktopAdapter {
  readonly #metadata: SnapshotMetadata;
  #nextBindingNumber = 1;
  #relations: RelationDto[];
  #bindings: Map<string, BindingDto>;

  constructor(snapshot: DesktopAdapterSnapshot = createManualRelationSnapshotFixture()) {
    super(snapshot);
    if (snapshot.metadata === null) {
      throw new Error("Manual relation fixture metadata is unavailable.");
    }
    this.#metadata = { ...snapshot.metadata };
    this.#relations = snapshot.relations.map(copyRelation);
    this.#bindings = new Map(
      snapshot.relations
        .filter((relation) => relation.evidence.type === "configured")
        .map((relation) => {
          if (relation.evidence.type !== "configured") {
            throw new Error("Manual relation fixture evidence is not configured.");
          }
          const binding = relation.evidence;
          const status = relation.lifecycle === "orphaned"
            ? "unresolved"
            : relation.lifecycle === "tombstoned"
              ? "disabled"
              : "active";
          return [
            binding.binding_id,
            configuredBinding(
              binding.binding_id,
              relation.source_resource_id,
              relation.target_resource_id,
              relation.kind,
              status,
            ),
          ] as const;
        }),
    );
  }

  override async searchResources(input: SearchResourcesInput = {}) {
    const page = await super.searchResources(input);
    const query = input.query?.trim().toLocaleLowerCase("en");
    const kinds = input.kinds === undefined ? null : new Set(input.kinds);
    const connectorTypes = input.connector_types === undefined ? null : new Set(input.connector_types);
    return {
      ...page,
      items: page.items.filter((item) => {
        const matchesQuery = query === undefined || query.length === 0 ||
          [item.resource_id, item.kind, item.display_name].some((value) => value.toLocaleLowerCase("en").includes(query));
        const connection = this.#connectionFor(item.connection_id);
        const matchesKind = kinds === null || kinds.has(item.kind);
        const matchesConnector = connectorTypes === null || (connection !== undefined && connectorTypes.has(connection.connector_type));
        return matchesQuery && matchesKind && matchesConnector;
      }),
    };
  }

  override async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    const detail = await super.getResource(input);
    return {
      ...detail,
      relations: this.#relations
        .filter((relation) => relation.source_resource_id === input.resource_id || relation.target_resource_id === input.resource_id)
        .map(copyRelation),
    };
  }

  override async getTopology(input: GetTopologyInput) {
    const topology = await super.getTopology(input);
    return {
      ...topology,
      edges: this.#relations.map(copyRelation),
    };
  }

  override async createBinding(input: CreateBindingInput) {
    this.#assertEndpointInput(input.source_resource_id, input.target_resource_id);
    if ([...this.#bindings.values()].some((binding) =>
      (binding.status === "active" || binding.status === "unresolved") &&
      binding.source_resource_id === input.source_resource_id &&
      binding.target_resource_id === input.target_resource_id &&
      binding.kind === input.kind,
    )) {
      throw Object.assign(new Error("Fixture binding already exists."), {
        code: "binding_conflict",
      });
    }
    const binding = configuredBinding(
      `fixture-binding-manual-${this.#nextBindingNumber++}`,
      input.source_resource_id,
      input.target_resource_id,
      input.kind,
      this.#statusForEndpoints(input.source_resource_id, input.target_resource_id),
    );
    this.#bindings.set(binding.binding_id, binding);
    this.#replaceRelation(binding);
    return this.#commandResult(binding) as unknown as Awaited<ReturnType<MockDesktopAdapter["createBinding"]>>;
  }

  override async updateBinding(input: UpdateBindingInput) {
    const existing = this.#bindings.get(input.binding_id);
    if (existing === undefined) throw new Error("Fixture binding was not found.");
    this.#assertEndpointInput(input.source_resource_id, input.target_resource_id);
    if (existing.status !== "disabled" && [...this.#bindings.values()].some((binding) =>
      binding.binding_id !== input.binding_id &&
      (binding.status === "active" || binding.status === "unresolved") &&
      binding.source_resource_id === input.source_resource_id &&
      binding.target_resource_id === input.target_resource_id &&
      binding.kind === input.kind,
    )) {
      throw Object.assign(new Error("Fixture binding already exists."), {
        code: "binding_conflict",
      });
    }
    const binding = {
      ...existing,
      source_resource_id: input.source_resource_id,
      target_resource_id: input.target_resource_id,
      kind: input.kind,
      status: existing.status === "disabled"
        ? "disabled"
        : this.#statusForEndpoints(input.source_resource_id, input.target_resource_id),
      updated_at: UPDATED_AT,
    } satisfies BindingDto;
    this.#bindings.set(binding.binding_id, binding);
    this.#replaceRelation(binding);
    return this.#commandResult(binding) as unknown as Awaited<ReturnType<MockDesktopAdapter["updateBinding"]>>;
  }

  override async disableBinding(input: DisableBindingInput) {
    const existing = this.#bindings.get(input.binding_id);
    if (existing === undefined) throw new Error("Fixture binding was not found.");
    const binding = { ...existing, status: "disabled" as const, updated_at: UPDATED_AT };
    this.#bindings.set(binding.binding_id, binding);
    this.#replaceRelation(binding);
    return this.#commandResult(binding) as unknown as Awaited<ReturnType<MockDesktopAdapter["disableBinding"]>>;
  }

  async getBinding(bindingId: string): Promise<BindingDto> {
    const binding = this.#bindings.get(bindingId);
    if (binding === undefined) throw new Error("Fixture binding was not found.");
    return copyBinding(binding);
  }

  #commandResult(binding: BindingDto): BindingCommandResultDto {
    return { metadata: { ...this.#metadata }, binding: copyBinding(binding) };
  }

  #replaceRelation(binding: BindingDto) {
    this.#relations = this.#relations.filter((relation) =>
      relation.evidence.type !== "configured" || relation.evidence.binding_id !== binding.binding_id,
    );
    this.#relations.push(relationFromBinding(binding));
  }

  #statusForEndpoints(sourceResourceId: string, targetResourceId: string): BindingDto["status"] {
    const source = this.#resourceFor(sourceResourceId);
    const target = this.#resourceFor(targetResourceId);
    return source?.lifecycle === "active" && target?.lifecycle === "active" ? "active" : "unresolved";
  }

  #assertEndpointInput(sourceResourceId: string, targetResourceId: string) {
    if (sourceResourceId === targetResourceId) throw new Error("Fixture binding endpoints must differ.");
    if (this.#resourceFor(sourceResourceId) === undefined) throw new Error("Fixture binding source was not found.");
  }

  #resourceFor(resourceId: string): ResourceDto | undefined {
    const snapshot = createManualRelationSnapshotFixture();
    return snapshot.resources.find((resource) => resource.resource_id === resourceId);
  }

  #connectionFor(connectionId: string): ConnectionDto | undefined {
    const snapshot = createManualRelationSnapshotFixture();
    return snapshot.connections.find((connection) => connection.connection_id === connectionId);
  }
}

export function createManualRelationAdapter(): ManualRelationAdapter {
  return new ManualRelationAdapter();
}
