import { useEffect, useMemo, useState } from "react";

import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { EvidenceSpine } from "../evidence/EvidenceSpine";

import "./resource-detail.css";

interface ResourceDetailPageProps { readonly resourceId: string; }

interface DetailState {
  readonly detail: ResourceDetailDto;
  readonly resources: ReadonlyMap<string, ResourceDto>;
}

function normalizedAttributes(value: unknown): readonly [string, string][] {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return [];
  return Object.entries(value)
    .filter((entry): entry is [string, string | number | boolean | null] =>
      entry[1] === null || ["string", "number", "boolean"].includes(typeof entry[1]),
    )
    .map(([key, item]) => [key, item === null ? "null" : String(item)]);
}

export function ResourceDetailPage({ resourceId }: ResourceDetailPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<DetailState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    adapter
      .getResource({
        resource_id: resourceId,
        include: ["attributes", "relations", "recent_changes", "connector_coverage"],
      })
      .then(async (detail) => {
        const endpointIds = new Set(
          detail.relations.flatMap((relation) => [
            relation.source_resource_id,
            relation.target_resource_id,
          ]),
        );
        endpointIds.add(detail.resource.resource_id);
        const endpoints = await Promise.all(
          [...endpointIds].map(async (id) => {
            if (id === detail.resource.resource_id) return detail.resource;
            return (await adapter.getResource({ resource_id: id })).resource;
          }),
        );
        if (!active) return;
        setState({ detail, resources: new Map(endpoints.map((item) => [item.resource_id, item])) });
      })
      .catch(() => {
        if (active) setError("Resource detail could not be loaded from the local snapshot.");
      });
    return () => { active = false; };
  }, [adapter, resourceId]);

  const evidenceGroups = useMemo(() => {
    if (state === null) return [];
    const groups = new Map<string, typeof state.detail.relations>();
    for (const relation of state.detail.relations) {
      const key = `${relation.source_resource_id}\u0000${relation.target_resource_id}`;
      groups.set(key, [...(groups.get(key) ?? []), relation]);
    }
    return [...groups.values()];
  }, [state]);

  if (error !== null) return <section className="resource-detail-state resource-detail-state--error" role="alert">{error}</section>;
  if (state === null) return <section className="resource-detail-state" aria-busy="true">Reading resource detail…</section>;

  const { detail } = state;
  const attributes = normalizedAttributes(detail.attributes);
  return (
    <div className="resource-detail-page">
      <header>
        <p className="resource-detail-eyebrow">Resource verification</p>
        <h1>{detail.resource.display_name}</h1>
        <code>{detail.resource.resource_id}</code>
      </header>

      <section className="resource-detail-facts" aria-label="Current resource facts">
        <div><small>Health</small><strong>{detail.resource.health}</strong></div>
        <div><small>Freshness</small><strong>{detail.resource.freshness}</strong></div>
        <div><small>Lifecycle</small><strong>{detail.resource.lifecycle}</strong></div>
        <div><small>Observed</small><time dateTime={detail.resource.observed_at}>{detail.resource.observed_at}</time></div>
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-evidence">
        <h2 id="detail-evidence">Evidence paths</h2>
        {evidenceGroups.length === 0 ? <p>No relations were included in this snapshot.</p> : evidenceGroups.map((relations) => {
          const source = state.resources.get(relations[0].source_resource_id);
          const target = state.resources.get(relations[0].target_resource_id);
          return source && target ? <EvidenceSpine key={`${source.resource_id}-${target.resource_id}`} relations={relations} source={source} target={target} /> : null;
        })}
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-attributes">
        <h2 id="detail-attributes">Normalized attributes</h2>
        {attributes.length === 0 ? <p>No normalized scalar attributes were included.</p> : <dl>{attributes.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>}
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-changes">
        <h2 id="detail-changes">Recent changes</h2>
        <p>{detail.recent_changes.length} structured changes</p>
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-coverage">
        <h2 id="detail-coverage">Connector coverage</h2>
        <p>{detail.connector_coverage.length} declared modules</p>
      </section>
    </div>
  );
}
