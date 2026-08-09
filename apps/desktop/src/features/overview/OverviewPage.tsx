import { useEffect, useMemo, useState } from "react";

import type { RouteId } from "../../app/routes";
import type { ChangeDto } from "../../generated/query/ChangeDto";
import type { HealthSummaryDto } from "../../generated/query/HealthSummaryDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { desktopErrorCode } from "../../platform/desktop-adapter/desktop-adapter";
import type { GitHubActionsSummarySnapshot } from "../../platform/desktop-adapter/desktop-adapter";

import "./overview.css";
import { formatRelativeTime } from "./time";

interface OverviewState {
  readonly healthSummary: HealthSummaryDto;
  readonly resources: readonly ResourceDto[];
  readonly changes: readonly ChangeDto[];
  readonly changesHasMore: boolean;
  readonly githubActionsSummary: GitHubActionsSummarySnapshot;
}

interface OverviewPageProps {
  readonly onInspectResource?: (resource: ResourceDto) => void;
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

type ConclusionTone = "fault" | "attention" | "healthy";

interface Conclusion {
  readonly tone: ConclusionTone;
  readonly text: string;
}

interface OverviewDerived {
  readonly totalResources: number;
  readonly totalConnections: number;
  readonly abnormalConnections: number;
  readonly attention: readonly AttentionItem[];
  readonly attentionCellTone: AttentionTone | "none";
  readonly conclusion: Conclusion;
  readonly githubTile: string | null;
}

function deriveOverview(state: OverviewState): OverviewDerived {
  const { resource_health, freshness, connector_health } = state.healthSummary;
  const totalResources =
    resource_health.healthy +
    resource_health.degraded +
    resource_health.unhealthy +
    resource_health.unknown;
  const totalConnections =
    connector_health.healthy +
    connector_health.degraded +
    connector_health.auth_failed +
    connector_health.rate_limited +
    connector_health.unreachable +
    connector_health.disabled;
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
      ? {
          tone: "fault",
          text: `有 ${resource_health.unhealthy} 个资源异常，需要优先处理。`,
        }
      : resource_health.degraded > 0 ||
          freshness.expired > 0 ||
          freshness.stale > 0 ||
          abnormalConnections > 0
        ? {
            tone: "attention",
            text:
              attention.length > 0
                ? `总体可用，有 ${attention.length} 个事项需要你留意。`
                : `总体可用，有 ${abnormalConnections} 个连接异常需要你留意。`,
          }
        : { tone: "healthy", text: "总体健康，没有需要处理的事项。" };

  let githubTotal = 0;
  let githubSucceeded = 0;
  let githubRunning = 0;
  for (const connection of state.githubActionsSummary.items) {
    for (const repository of connection.repositories) {
      githubTotal += repository.action_count;
      githubSucceeded += repository.succeeded;
      githubRunning += repository.running;
    }
  }
  const githubTile =
    state.githubActionsSummary.items.length === 0
      ? null
      : githubTotal > 0
        ? `GitHub Actions · 通过率 ${Math.round((githubSucceeded / githubTotal) * 100)}% · ${githubRunning} 运行中`
        : `GitHub Actions · 通过率 ${githubSucceeded}/${githubTotal} · ${githubRunning} 运行中`;

  return {
    totalResources,
    totalConnections,
    abnormalConnections,
    attention,
    attentionCellTone,
    conclusion,
    githubTile,
  };
}

export function OverviewPage({ onInspectResource, onNavigate, queryVersion = 0 }: OverviewPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<OverviewState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([
      adapter.getHealthSummary(),
      adapter.searchResources({ limit: 25 }),
      adapter.getRecentChanges({ limit: 20 }),
      adapter.getGitHubActionsSummary(),
    ])
      .then(([healthSummary, resourcePage, changePage, githubActionsSummary]) => {
        if (!active) return;
        setState({
          healthSummary,
          resources: resourcePage.items,
          changes: changePage.items,
          changesHasMore: changePage.page_info.next_cursor !== null,
          githubActionsSummary,
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
        <span>正在加载受限资源、连接和近期变更。</span>
      </section>
    );
  }

  const headerCount = `共 ${derived.totalResources} 个资源`;
  const resourceStat = `${derived.totalResources} 个资源`;
  const attentionStat = `${derived.attention.length} 条事项`;
  const connectionStat = `${derived.totalConnections} 个连接`;
  const abnormalStat = `${derived.abnormalConnections} 异常`;
  const snapshotStat = formatRelativeTime(state.healthSummary.metadata.generated_at);

  const inventoryTile = `资源清单 · 共 ${derived.totalResources} 个资源`;
  const connectorsTile = `连接器 · ${derived.totalConnections} 个连接`;
  const timelineTile = `时间线 · ${state.changes.length} 项变更${state.changesHasMore ? "+" : ""}`;

  return (
    <div className="overview-page">
      <header className="overview-header">
        <div>
          <p className="overview-eyebrow">本地核验路径</p>
          <h1>概览</h1>
          <p>在打开提供方控制台前，先找到需要检查的事实。</p>
        </div>
        <span className="overview-snapshot-count">{headerCount}</span>
      </header>

      <section className="overview-summary" aria-label="本地状态摘要">
        <div className="overview-summary-stats">
          <div className="overview-summary-cell">
            <span className="overview-summary-label">资源</span>
            <strong>{resourceStat}</strong>
          </div>
          <div className={`overview-summary-cell overview-summary-cell--${derived.attentionCellTone}`}>
            <span className="overview-summary-label">需关注</span>
            <strong>{attentionStat}</strong>
          </div>
          <div
            className={`overview-summary-cell${derived.abnormalConnections > 0 ? " overview-summary-cell--fault" : ""}`}
          >
            <span className="overview-summary-label">连接</span>
            <strong>{connectionStat}</strong>
            {derived.abnormalConnections > 0 ? <small>{abnormalStat}</small> : null}
          </div>
          <div className="overview-summary-cell">
            <span className="overview-summary-label">上次快照</span>
            <strong>{snapshotStat}</strong>
          </div>
        </div>
        <p className={`overview-conclusion overview-conclusion--${derived.conclusion.tone}`}>
          <span className="overview-conclusion-dot" aria-hidden="true" />
          {derived.conclusion.text}
        </p>
      </section>

      <section className="overview-section" aria-labelledby="overview-attention">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">优先核验</p>
            <h2 id="overview-attention">需要关注</h2>
          </div>
        </div>
        {state.resources.length < derived.totalResources ? (
          <p className="overview-truncation-note">仅基于前 25 个资源计算。</p>
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
              <button
                className={`overview-attention-row overview-attention-row--${item.tone}`}
                key={item.resource.resource_id}
                onClick={() => onInspectResource?.(item.resource)}
                type="button"
              >
                <span className="overview-attention-marker" aria-hidden="true" />
                <span className="overview-attention-identity">
                  <strong>{item.resource.display_name}</strong>
                  <code>{item.resource.kind}</code>
                </span>
                <span className={`overview-attention-badge overview-attention-badge--${item.tone}`}>
                  {item.badge}
                </span>
                <span className="overview-attention-reason">
                  {item.reason}
                  {" "}
                  <time dateTime={item.resource.observed_at} title={item.resource.observed_at}>
                    {formatRelativeTime(item.resource.observed_at)}
                  </time>
                </span>
                <span className="overview-attention-chevron" aria-hidden="true">
                  ›
                </span>
              </button>
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
          <span>{inventoryTile}</span>
          <span aria-hidden="true">›</span>
        </button>
        <button
          className="overview-nav-tile"
          onClick={() => onNavigate?.("connectors")}
          type="button"
        >
          <span>
            {connectorsTile}
            {derived.abnormalConnections > 0 ? (
              <span className="overview-nav-tile-abnormal"> · {derived.abnormalConnections} 异常</span>
            ) : null}
          </span>
          <span aria-hidden="true">›</span>
        </button>
        <button
          className="overview-nav-tile"
          onClick={() => onNavigate?.("timeline")}
          type="button"
        >
          <span>{timelineTile}</span>
          <span aria-hidden="true">›</span>
        </button>
        {derived.githubTile === null ? null : (
          <button
            className="overview-nav-tile"
            onClick={() => onNavigate?.("connectors")}
            type="button"
          >
            <span>{derived.githubTile}</span>
            <span aria-hidden="true">›</span>
          </button>
        )}
      </nav>
    </div>
  );
}
