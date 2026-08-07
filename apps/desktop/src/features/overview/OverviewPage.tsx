import { useEffect, useMemo, useState } from "react";

import type { ChangeDto } from "../../generated/query/ChangeDto";
import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { GitHubActionsSummarySnapshot } from "../../platform/desktop-adapter/desktop-adapter";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { desktopErrorCode } from "../../platform/desktop-adapter/desktop-adapter";

import "./overview.css";

interface OverviewState {
  readonly resources: readonly ResourceDto[];
  readonly connections: readonly ConnectionDto[];
  readonly changes: readonly ChangeDto[];
  readonly githubActionsSummary: GitHubActionsSummarySnapshot;
}

interface OverviewPageProps {
  readonly onInspectResource?: (resource: ResourceDto) => void;
}

function attentionReason(resource: ResourceDto): string | null {
  if (resource.health === "unhealthy") return "资源报告为不健康";
  if (resource.health === "degraded") return "资源报告为降级";
  if (resource.freshness === "expired") return "已保存事实已过期";
  if (resource.freshness === "stale") return "已保存事实已过时";
  if (resource.lifecycle !== "active") return `生命周期为 ${displayEnum(resource.lifecycle)}`;
  return null;
}

function attentionTone(resource: ResourceDto): string {
  if (resource.health === "unhealthy") return "fault";
  if (resource.freshness === "expired" || resource.health === "degraded") return "inspect";
  return "unknown";
}

export function OverviewPage({ onInspectResource }: OverviewPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<OverviewState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([
      adapter.searchResources({ limit: 25 }),
      adapter.listConnections(),
      adapter.getRecentChanges({ limit: 8 }),
      adapter.getGitHubActionsSummary(),
    ])
      .then(([resourcePage, connectionsSnapshot, changePage, githubActionsSummary]) => {
        if (!active) return;
        setState({
          resources: resourcePage.items,
          connections: connectionsSnapshot.items,
          changes: changePage.items,
          githubActionsSummary,
        });
      })
      .catch((error) => {
        if (active) setError(`无法查询本地快照（${desktopErrorCode(error)}）。`);
      });
    return () => {
      active = false;
    };
  }, [adapter]);

  const attention = useMemo(
    () =>
      state?.resources
        .filter((resource) => resource.kind !== "github.workflow_run")
        .map((resource) => ({ resource, reason: attentionReason(resource) }))
        .filter(
          (item): item is { resource: ResourceDto; reason: string } =>
            item.reason !== null,
        ) ?? [],
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

  if (state === null) {
    return (
      <section className="overview-state" aria-busy="true">
        <strong>正在读取本地快照</strong>
        <span>正在加载受限资源、连接和近期变更。</span>
      </section>
    );
  }

  return (
    <div className="overview-page">
      <header className="overview-header">
        <div>
          <p className="overview-eyebrow">本地核验路径</p>
          <h1>概览</h1>
          <p>在打开提供方控制台前，先找到需要检查的事实。</p>
        </div>
        <span className="overview-snapshot-count">
          {state.resources.length} 个受限资源
        </span>
      </header>

      <section className="overview-section" aria-labelledby="overview-attention">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">优先核验</p>
            <h2 id="overview-attention">关注队列</h2>
          </div>
          <span>{attention.length} 条事实</span>
        </div>
        {attention.length === 0 ? (
          <p className="overview-empty">此页面没有不健康、过期、过时或非活动的事实。</p>
        ) : (
          <div className="overview-attention-list">
            {attention.map(({ resource, reason }) => (
              <button
                className={`overview-attention-row overview-attention-row--${attentionTone(resource)}`}
                key={resource.resource_id}
                onClick={() => onInspectResource?.(resource)}
                type="button"
              >
                <span className="overview-attention-marker" aria-hidden="true" />
                <span className="overview-attention-identity">
                  <strong>{resource.display_name}</strong>
                  <code>{resource.kind}</code>
                </span>
                <span className="overview-attention-reason">{reason}</span>
                <span className="overview-attention-facts">
                  <small>健康度</small> {displayEnum(resource.health)}
                  <small>新鲜度</small> {displayEnum(resource.freshness)}
                </span>
                <time dateTime={resource.observed_at}>{resource.observed_at}</time>
              </button>
            ))}
          </div>
        )}
      </section>

      <section className="overview-section" aria-labelledby="overview-observations">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">连接器心跳</p>
            <h2 id="overview-observations">观测概览</h2>
          </div>
          <span>{state.connections.length} 个连接</span>
        </div>
        <div className="overview-observation-strip">
          {state.connections.map((connection) => (
            <article key={connection.connection_id}>
              <span className={`overview-connection-state state-${connection.health}`}>
                {displayEnum(connection.health)}
              </span>
              <strong>{connection.display_name}</strong>
              <code>{connection.connector_type}</code>
              <dl>
                <div><dt>最近成功</dt><dd>{connection.last_success_at ?? "从未"}</dd></div>
                <div><dt>最近尝试</dt><dd>{connection.last_attempt_at ?? "从未"}</dd></div>
              </dl>
            </article>
          ))}
        </div>
      </section>

      <section className="overview-section" aria-labelledby="overview-github-actions">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">GitHub CI/CD</p>
            <h2 id="overview-github-actions">GitHub Actions 聚合</h2>
          </div>
        </div>
        {state.githubActionsSummary.items.length === 0 ? (
          <p className="overview-empty">没有已同步的 GitHub Actions 数据。</p>
        ) : (
          <div className="overview-github-actions-table">
            {state.githubActionsSummary.items.map((connection) => (
              <div key={connection.connection_id} className="overview-github-actions-connection">
                <h3>{connection.connection_name}</h3>
                <table>
                  <thead>
                    <tr>
                      <th>仓库</th>
                      <th>Action 数量</th>
                      <th>成功</th>
                      <th>失败</th>
                      <th>进行中</th>
                    </tr>
                  </thead>
                  <tbody>
                    {connection.repositories.map((repo) => (
                      <tr key={repo.repository_id}>
                        <td>{repo.repository_name}</td>
                        <td>{repo.action_count}</td>
                        <td className="overview-github-actions--success">{repo.succeeded}</td>
                        <td className="overview-github-actions--failed">{repo.failed}</td>
                        <td className="overview-github-actions--running">{repo.running}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="overview-section overview-critical" aria-labelledby="overview-critical">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">仅限已配置路径</p>
            <h2 id="overview-critical">关键路径</h2>
          </div>
        </div>
        <p>
          当前没有固定关键路径。Next Infra 不会根据展示名称或近期活动推断重要性。
        </p>
      </section>

      <section className="overview-section" aria-labelledby="overview-changes">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">结构化差异</p>
            <h2 id="overview-changes">近期变更</h2>
          </div>
          <span>{state.changes.length} 项变更</span>
        </div>
        {state.changes.length === 0 ? (
          <p className="overview-empty">受限查询中没有结构化变更。</p>
        ) : (
          <ol className="overview-change-list">
            {state.changes.map((change) => (
              <li key={change.change_id}>
                <time dateTime={change.observed_at}>{change.observed_at}</time>
                <code>{change.change_id}</code>
                <span>{change.fields.length} 个变更字段</span>
              </li>
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}
