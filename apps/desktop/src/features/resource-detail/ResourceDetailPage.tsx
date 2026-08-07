import { useEffect, useMemo, useState } from "react";

import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
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
        if (active) setError("无法从本地快照加载资源详情。");
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
  if (state === null) return <section className="resource-detail-state" aria-busy="true">正在读取资源详情…</section>;

  const { detail } = state;
  const attributes = normalizedAttributes(detail.attributes);
  return (
    <div className="resource-detail-page">
      <header>
        <p className="resource-detail-eyebrow">资源核验</p>
        <h1>{detail.resource.display_name}</h1>
        <code>{detail.resource.resource_id}</code>
      </header>

      <section className="resource-detail-facts" aria-label="当前资源事实">
        <div><small>健康度</small><strong>{displayEnum(detail.resource.health)}</strong></div>
        <div><small>新鲜度</small><strong>{displayEnum(detail.resource.freshness)}</strong></div>
        <div><small>生命周期</small><strong>{displayEnum(detail.resource.lifecycle)}</strong></div>
        <div><small>观测时间</small><time dateTime={detail.resource.observed_at}>{detail.resource.observed_at}</time></div>
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-evidence">
        <h2 id="detail-evidence">证据路径</h2>
        {evidenceGroups.length === 0 ? <p>此快照未包含关系。</p> : evidenceGroups.map((relations) => {
          const source = state.resources.get(relations[0].source_resource_id);
          const target = state.resources.get(relations[0].target_resource_id);
          return source && target ? <EvidenceSpine key={`${source.resource_id}-${target.resource_id}`} relations={relations} source={source} target={target} /> : null;
        })}
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-attributes">
        <h2 id="detail-attributes">规范化属性</h2>
        {attributes.length === 0 ? <p>未包含规范化的标量属性。</p> : <dl>{attributes.map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl>}
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-changes">
        <h2 id="detail-changes">近期变更</h2>
        <p>{detail.recent_changes.length} 项结构化变更</p>
      </section>

      <section className="resource-detail-section" aria-labelledby="detail-coverage">
        <h2 id="detail-coverage">连接器覆盖范围</h2>
        <p>{detail.connector_coverage.length} 个声明模块</p>
      </section>
    </div>
  );
}
