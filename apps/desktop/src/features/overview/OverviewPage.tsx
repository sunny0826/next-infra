import { useEffect, useId, useMemo, useState } from "react";

import type { RouteId } from "../../app/routes";
import type { HealthSummaryDto } from "../../generated/query/HealthSummaryDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import { humanizeKind } from "../../lib/format";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { desktopErrorCode } from "../../platform/desktop-adapter/desktop-adapter";
import type { DesktopAdapter } from "../../platform/desktop-adapter/desktop-adapter";

import "./overview.css";
import { formatRelativeTime } from "./time";

const RESOURCE_PAGE_LIMIT = 25;

interface OverviewState {
  readonly healthSummary: HealthSummaryDto;
  readonly resources: readonly ResourceDto[];
  readonly resourcesTruncated: boolean;
}

interface OverviewPageProps {
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
  readonly onNavigate?: (routeId: RouteId) => void;
  readonly queryVersion?: number;
}

type AttentionTone = "fault" | "inspect" | "unknown";

interface AttentionItem {
  readonly resource: ResourceDto;
  readonly severity: number;
  readonly tone: AttentionTone;
  readonly badge: string;
  readonly reason: string;
}

function buildAttentionItem(resource: ResourceDto): AttentionItem | null {
  if (resource.health === "unhealthy") {
    return {
      resource,
      severity: 0,
      tone: "fault",
      badge: displayEnum(resource.health),
      reason: "报告不健康",
    };
  }
  if (resource.freshness === "expired") {
    return {
      resource,
      severity: 1,
      tone: "inspect",
      badge: displayEnum(resource.freshness),
      reason: "最后更新",
    };
  }
  if (resource.health === "degraded") {
    return {
      resource,
      severity: 2,
      tone: "inspect",
      badge: displayEnum(resource.health),
      reason: "状态降级",
    };
  }
  if (resource.freshness === "stale") {
    return {
      resource,
      severity: 3,
      tone: "unknown",
      badge: displayEnum(resource.freshness),
      reason: "数据较旧 · 最后更新",
    };
  }
  if (resource.lifecycle !== "active") {
    return {
      resource,
      severity: 4,
      tone: "unknown",
      badge: displayEnum(resource.lifecycle),
      reason: `生命周期为 ${displayEnum(resource.lifecycle)}`,
    };
  }
  return null;
}

interface EvidencePair {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

type ItemEvidence =
  | { readonly status: "loading" }
  | { readonly status: "error" }
  | { readonly status: "ready"; readonly pairs: readonly EvidencePair[] };

async function resolveEndpoint(
  resourceId: string,
  resourcesById: ReadonlyMap<string, ResourceDto>,
  adapter: DesktopAdapter,
): Promise<ResourceDto | null> {
  const known = resourcesById.get(resourceId);
  if (known !== undefined) return known;
  try {
    return (await adapter.getResource({ resource_id: resourceId })).resource;
  } catch {
    return null;
  }
}

/**
 * Evidence panel for one attention item. Mounts only while the item is the
 * expanded one, so closed rows never trigger relation reads.
 */
function AttentionEvidence({
  item,
  resourcesById,
  onInspectRelation,
  evidenceId,
}: {
  readonly item: AttentionItem;
  readonly resourcesById: ReadonlyMap<string, ResourceDto>;
  readonly onInspectRelation?: (relation: RelationDto) => void;
  readonly evidenceId: string;
}) {
  const adapter = useDesktopAdapter();
  const [evidence, setEvidence] = useState<ItemEvidence>({ status: "loading" });

  useEffect(() => {
    let active = true;
    setEvidence({ status: "loading" });
    adapter
      .getResource({ resource_id: item.resource.resource_id, include: ["relations"] })
      .then(async (detail) => {
        const groups = new Map<string, RelationDto[]>();
        for (const relation of detail.relations) {
          const key = `${relation.source_resource_id}\u0000${relation.target_resource_id}`;
          const group = groups.get(key);
          if (group === undefined) {
            groups.set(key, [relation]);
          } else {
            group.push(relation);
          }
        }
        const pairs: EvidencePair[] = [];
        for (const [key, relations] of groups) {
          const [sourceId, targetId] = key.split("\u0000");
          if (sourceId === undefined || targetId === undefined) continue;
          const [source, target] = await Promise.all([
            resolveEndpoint(sourceId, resourcesById, adapter),
            resolveEndpoint(targetId, resourcesById, adapter),
          ]);
          if (source !== null && target !== null) {
            pairs.push({ source, target, relations });
          }
        }
        if (!active) return;
        setEvidence({ status: "ready", pairs });
      })
      .catch(() => {
        if (active) setEvidence({ status: "error" });
      });
    return () => {
      active = false;
    };
  }, [adapter, item.resource.resource_id, resourcesById]);

  if (evidence.status === "loading") {
    return (
      <p className="overview-attention-evidence-loading" id={evidenceId}>
        正在读取证据…
      </p>
    );
  }
  if (evidence.status === "error") {
    return (
      <p className="overview-attention-evidence-error" id={evidenceId}>
        无法读取证据。
      </p>
    );
  }
  if (evidence.pairs.length === 0) {
    return (
      <p className="overview-attention-evidence-empty" id={evidenceId}>
        未发现关联证据。
      </p>
    );
  }
  return (
    <div className="overview-attention-evidence" id={evidenceId}>
      {evidence.pairs.map((pair) => {
        const representative = pair.relations[0];
        return (
          <button
            key={`${pair.source.resource_id}\u0000${pair.target.resource_id}`}
            className="overview-evidence-summary"
            disabled={onInspectRelation === undefined}
            onClick={() => onInspectRelation?.(representative)}
            title={onInspectRelation === undefined ? "检查器不可用" : undefined}
            type="button"
          >
            <span className="overview-evidence-summary-path">
              <strong>{pair.source.display_name}</strong>
              <span aria-hidden="true">→</span>
              <strong>{pair.target.display_name}</strong>
            </span>
            <span className="overview-evidence-summary-meta">
              {pair.relations.length} 条关系 · {humanizeKind(representative.kind)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

type ConclusionTone = "fault" | "attention" | "healthy";

interface Conclusion {
  readonly tone: ConclusionTone;
  readonly text: string;
}

interface OverviewDerived {
  readonly abnormalConnections: number;
  readonly attention: readonly AttentionItem[];
  readonly attentionCellTone: AttentionTone | "none";
  readonly conclusion: Conclusion;
}

function deriveOverview(state: OverviewState): OverviewDerived {
  const { resource_health, freshness, connector_health } = state.healthSummary;
  const abnormalConnections =
    connector_health.degraded +
    connector_health.auth_failed +
    connector_health.rate_limited +
    connector_health.unreachable;

  const attention = state.resources
    .filter((resource) => resource.kind !== "github.workflow_run")
    .map((resource) => buildAttentionItem(resource))
    .filter((item): item is AttentionItem => item !== null)
    .sort((left, right) => left.severity - right.severity);

  const attentionCellTone: AttentionTone | "none" = attention.some(
    (item) => item.tone === "fault",
  )
    ? "fault"
    : attention.some((item) => item.tone === "inspect")
      ? "inspect"
      : attention.length === 0
        ? "none"
        : "unknown";

  const conclusion: Conclusion =
    resource_health.unhealthy > 0
      ? { tone: "fault", text: `有 ${resource_health.unhealthy} 个资源异常，需要优先处理。` }
      : resource_health.degraded > 0
        ? { tone: "attention", text: `有 ${resource_health.degraded} 个资源处于降级状态。` }
        : freshness.expired > 0 || freshness.stale > 0
          ? {
              tone: "attention",
              text: `有 ${freshness.expired + freshness.stale} 个资源的观察数据过期或较旧。`,
            }
          : abnormalConnections > 0
            ? { tone: "attention", text: `有 ${abnormalConnections} 个连接异常，观测链路不完整。` }
            : { tone: "healthy", text: "总体健康，没有待处理事项。" };

  return { abnormalConnections, attention, attentionCellTone, conclusion };
}

function AttentionRow({
  item,
  resourcesById,
  expanded,
  onToggleEvidence,
  onInspectResource,
  onInspectRelation,
}: {
  readonly item: AttentionItem;
  readonly resourcesById: ReadonlyMap<string, ResourceDto>;
  readonly expanded: boolean;
  readonly onToggleEvidence: () => void;
  readonly onInspectResource?: (resource: ResourceDto) => void;
  readonly onInspectRelation?: (relation: RelationDto) => void;
}) {
  const evidenceId = useId();
  return (
    <div className="overview-attention-item">
      <div className={`overview-attention-row overview-attention-row--${item.tone}`}>
        <span className="overview-attention-marker" aria-hidden="true" />
        <span className="overview-attention-identity">
          <strong>{item.resource.display_name}</strong>
          <code>{item.resource.kind}</code>
        </span>
        <span className={`overview-attention-badge overview-attention-badge--${item.tone}`}>
          {item.badge}
        </span>
        <span className="overview-attention-reason">
          {item.reason}{" "}
          <time dateTime={item.resource.observed_at} title={item.resource.observed_at}>
            {formatRelativeTime(item.resource.observed_at)}
          </time>
        </span>
        <span className="overview-attention-actions">
          <button
            className="overview-attention-action"
            onClick={() => onInspectResource?.(item.resource)}
            type="button"
          >
            查看资源
          </button>
          <button
            className="overview-attention-action"
            aria-controls={evidenceId}
            aria-expanded={expanded}
            onClick={onToggleEvidence}
            type="button"
          >
            {expanded ? "收起证据" : "核验证据"}
          </button>
        </span>
      </div>
      {expanded ? (
        <AttentionEvidence
          item={item}
          resourcesById={resourcesById}
          onInspectRelation={onInspectRelation}
          evidenceId={evidenceId}
        />
      ) : null}
    </div>
  );
}

export function OverviewPage({ onInspectResource, onInspectRelation, onNavigate, queryVersion = 0 }: OverviewPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<OverviewState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expandedResourceId, setExpandedResourceId] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([
      adapter.getHealthSummary(),
      adapter.searchResources({ limit: RESOURCE_PAGE_LIMIT }),
    ])
      .then(([healthSummary, resourcePage]) => {
        if (!active) return;
        const totalResources =
          healthSummary.resource_health.healthy +
          healthSummary.resource_health.degraded +
          healthSummary.resource_health.unhealthy +
          healthSummary.resource_health.unknown;
        setState({
          healthSummary,
          resources: resourcePage.items,
          resourcesTruncated:
            resourcePage.page_info.next_cursor !== null ||
            resourcePage.items.length < totalResources,
        });
      })
      .catch((error) => {
        if (active) setError(`无法查询本地快照（${desktopErrorCode(error)}）。`);
      });
    return () => {
      active = false;
    };
  }, [adapter, queryVersion]);

  const derived = useMemo(
    () => (state === null ? null : deriveOverview(state)),
    [state],
  );
  const resourcesById = useMemo(
    () => new Map((state?.resources ?? []).map((resource) => [resource.resource_id, resource] as const)),
    [state],
  );

  if (error !== null) {
    return (
      <section className="overview-state overview-state--error" role="alert">
        <strong>概览不可用</strong>
        <span>{error}</span>
      </section>
    );
  }

  if (state === null || derived === null) {
    return (
      <section className="overview-state" aria-busy="true">
        <strong>正在读取本地快照</strong>
        <span>正在加载状态摘要和受限资源。</span>
      </section>
    );
  }

  const attentionStat = state.resourcesTruncated
    ? `前 ${RESOURCE_PAGE_LIMIT} 个资源中 ${derived.attention.length} 项`
    : `${derived.attention.length} 项`;
  const abnormalStat = `${derived.abnormalConnections}`;
  const snapshotStat = formatRelativeTime(state.healthSummary.metadata.generated_at);

  return (
    <div className="overview-page">
      <header className="overview-header">
        <div>
          <p className="overview-eyebrow">本地核验路径</p>
          <h1>概览</h1>
          <p>在打开提供方控制台前，先找到需要检查的事实。</p>
        </div>
      </header>

      <section className="overview-summary" aria-label="本地状态摘要">
        <p className={`overview-conclusion overview-conclusion--${derived.conclusion.tone}`}>
          <span className="overview-conclusion-dot" aria-hidden="true" />
          {derived.conclusion.text}
        </p>
        <div className="overview-summary-stats">
          <div className={`overview-summary-cell overview-summary-cell--${derived.attentionCellTone}`}>
            <span className="overview-summary-label">待核验</span>
            <strong>{attentionStat}</strong>
          </div>
          <div
            className={`overview-summary-cell${derived.abnormalConnections > 0 ? " overview-summary-cell--fault" : ""}`}
          >
            <span className="overview-summary-label">异常连接</span>
            <strong>{abnormalStat}</strong>
          </div>
          <div className="overview-summary-cell">
            <span className="overview-summary-label">快照</span>
            <strong>{snapshotStat}</strong>
          </div>
        </div>
      </section>

      <section className="overview-section" aria-labelledby="overview-attention">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">优先核验</p>
            <h2 id="overview-attention">需要关注</h2>
          </div>
        </div>
        {state.resourcesTruncated ? (
          <p className="overview-truncation-note">
            资源页被截断：待核验仅覆盖前 {RESOURCE_PAGE_LIMIT} 个资源，不代表全局总数。
          </p>
        ) : null}
        {derived.attention.length === 0 ? (
          <p className="overview-empty">
            没有需要关注的事项。
            <button
              className="overview-inline-link"
              onClick={() => onNavigate?.("inventory")}
              type="button"
            >
              查看全部资源
            </button>
          </p>
        ) : (
          <div className="overview-attention-list">
            {derived.attention.map((item) => (
              <AttentionRow
                key={item.resource.resource_id}
                item={item}
                resourcesById={resourcesById}
                expanded={expandedResourceId === item.resource.resource_id}
                onToggleEvidence={() =>
                  setExpandedResourceId((current) =>
                    current === item.resource.resource_id ? null : item.resource.resource_id,
                  )
                }
                onInspectResource={onInspectResource}
                onInspectRelation={onInspectRelation}
              />
            ))}
          </div>
        )}
      </section>

      <nav className="overview-quicknav" aria-label="快速导航">
        <button
          className="overview-nav-tile"
          onClick={() => onNavigate?.("inventory")}
          type="button"
        >
          <span>查看全部资源</span>
          <span aria-hidden="true">›</span>
        </button>
        <button
          className="overview-nav-tile"
          onClick={() => onNavigate?.("connectors")}
          type="button"
        >
          <span>查看异常连接</span>
          <span aria-hidden="true">›</span>
        </button>
        <button
          className="overview-nav-tile"
          onClick={() => onNavigate?.("timeline")}
          type="button"
        >
          <span>查看最近变更</span>
          <span aria-hidden="true">›</span>
        </button>
      </nav>
    </div>
  );
}
