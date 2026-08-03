import { useEffect, useMemo, useState } from "react";

import type { ChangeDto } from "../../generated/query/ChangeDto";
import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./overview.css";

interface OverviewState {
  readonly resources: readonly ResourceDto[];
  readonly connections: readonly ConnectionDto[];
  readonly changes: readonly ChangeDto[];
}

interface OverviewPageProps {
  readonly onInspectResource?: (resource: ResourceDto) => void;
}

function attentionReason(resource: ResourceDto): string | null {
  if (resource.health === "unhealthy") return "Resource reports unhealthy";
  if (resource.health === "degraded") return "Resource reports degraded";
  if (resource.freshness === "expired") return "Saved fact is expired";
  if (resource.freshness === "stale") return "Saved fact is stale";
  if (resource.lifecycle !== "active") return `Lifecycle is ${resource.lifecycle}`;
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
    ])
      .then(([resourcePage, connections, changePage]) => {
        if (!active) return;
        setState({
          resources: resourcePage.items,
          connections,
          changes: changePage.items,
        });
      })
      .catch(() => {
        if (active) setError("The local snapshot could not be queried.");
      });
    return () => {
      active = false;
    };
  }, [adapter]);

  const attention = useMemo(
    () =>
      state?.resources
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
        <strong>Overview unavailable</strong>
        <span>{error}</span>
      </section>
    );
  }

  if (state === null) {
    return (
      <section className="overview-state" aria-busy="true">
        <strong>Reading local snapshot</strong>
        <span>Loading bounded resources, connections, and recent changes.</span>
      </section>
    );
  }

  return (
    <div className="overview-page">
      <header className="overview-header">
        <div>
          <p className="overview-eyebrow">Local verification path</p>
          <h1>Overview</h1>
          <p>Find facts that need inspection before opening a provider console.</p>
        </div>
        <span className="overview-snapshot-count">
          {state.resources.length} bounded resources
        </span>
      </header>

      <section className="overview-section" aria-labelledby="overview-attention">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">Verify first</p>
            <h2 id="overview-attention">Attention queue</h2>
          </div>
          <span>{attention.length} facts</span>
        </div>
        {attention.length === 0 ? (
          <p className="overview-empty">No unhealthy, expired, stale, or inactive facts in this page.</p>
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
                  <small>Health</small> {resource.health}
                  <small>Freshness</small> {resource.freshness}
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
            <p className="overview-eyebrow">Connector heartbeat</p>
            <h2 id="overview-observations">Observation strip</h2>
          </div>
          <span>{state.connections.length} connections</span>
        </div>
        <div className="overview-observation-strip">
          {state.connections.map((connection) => (
            <article key={connection.connection_id}>
              <span className={`overview-connection-state state-${connection.health}`}>
                {connection.health}
              </span>
              <strong>{connection.display_name}</strong>
              <code>{connection.connector_type}</code>
              <dl>
                <div><dt>Last success</dt><dd>{connection.last_success_at ?? "never"}</dd></div>
                <div><dt>Last attempt</dt><dd>{connection.last_attempt_at ?? "never"}</dd></div>
              </dl>
            </article>
          ))}
        </div>
      </section>

      <section className="overview-section overview-critical" aria-labelledby="overview-critical">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">Configured paths only</p>
            <h2 id="overview-critical">Critical paths</h2>
          </div>
        </div>
        <p>
          No critical path is pinned. Next Infra will not infer importance from display names or
          recent activity.
        </p>
      </section>

      <section className="overview-section" aria-labelledby="overview-changes">
        <div className="overview-section-heading">
          <div>
            <p className="overview-eyebrow">Structured differences</p>
            <h2 id="overview-changes">Recent changes</h2>
          </div>
          <span>{state.changes.length} changes</span>
        </div>
        {state.changes.length === 0 ? (
          <p className="overview-empty">No structured changes in the bounded query.</p>
        ) : (
          <ol className="overview-change-list">
            {state.changes.map((change) => (
              <li key={change.change_id}>
                <time dateTime={change.observed_at}>{change.observed_at}</time>
                <code>{change.change_id}</code>
                <span>{change.fields.length} changed fields</span>
              </li>
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}
