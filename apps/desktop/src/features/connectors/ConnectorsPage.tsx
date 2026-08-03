import { useEffect, useState } from "react";

import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ConnectorCoverageDto } from "../../generated/query/ConnectorCoverageDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./connectors.css";

interface ConnectionRow {
  readonly connection: ConnectionDto;
  readonly nextScheduledAt: string | null;
  readonly recentStatus: string | null;
}

export function ConnectorsPage() {
  const adapter = useDesktopAdapter();
  const [rows, setRows] = useState<readonly ConnectionRow[] | null>(null);
  const [coverage, setCoverage] = useState<readonly ConnectorCoverageDto[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([adapter.listConnections(), adapter.listConnectorCoverage()])
      .then(async ([connections, coverageSnapshot]) => {
        const statuses = await Promise.all(
          connections.map(async (connection) => {
            const status = await adapter.getSyncStatus({ connection_id: connection.connection_id, recent_run_limit: 1 });
            return {
              connection,
              nextScheduledAt: status.next_scheduled_at,
              recentStatus: status.recent_runs[0]?.status ?? null,
            };
          }),
        );
        if (!active) return;
        setRows(statuses);
        setCoverage(coverageSnapshot.items);
      })
      .catch(() => { if (active) setError("Connector state could not be queried."); });
    return () => { active = false; };
  }, [adapter]);

  async function startManualSync(connection: ConnectionDto) {
    setNotice(null);
    try {
      const result = await adapter.manualSync(connection.connection_id);
      setNotice(`Manual Sync queued as ${result.sync_run_id}. The current page was not refreshed.`);
    } catch {
      setNotice("Manual Sync could not be queued. Existing snapshot facts remain unchanged.");
    }
  }

  if (error !== null) return <section className="connectors-state connectors-state--error" role="alert">{error}</section>;
  if (rows === null) return <section className="connectors-state" aria-busy="true">Reading connector state…</section>;

  return (
    <div className="connectors-page">
      <header><div><p className="connectors-eyebrow">Observation transport</p><h1>Connectors</h1><p>Verify read health and declared coverage without conflating them with resource health.</p></div><span>{rows.length} local connections</span></header>
      {notice ? <div className="connectors-notice" role="status">{notice}</div> : null}
      <section className="connectors-section" aria-labelledby="connection-state"><div><h2 id="connection-state">Connection state</h2><span>Manual Sync is separate from refresh</span></div>
        <div className="connectors-frame"><table><thead><tr><th>Connection</th><th>Health</th><th>Last success</th><th>Last attempt</th><th>Recent run</th><th>Next scheduled</th><th>Action</th></tr></thead><tbody>
          {rows.map(({ connection, nextScheduledAt, recentStatus }) => <tr key={connection.connection_id}>
            <td><strong>{connection.display_name}</strong><code>{connection.connector_type}</code></td>
            <td><span className={`connectors-status state-${connection.health}`}>{connection.health}</span></td>
            <td><time>{connection.last_success_at ?? "never"}</time></td><td><time>{connection.last_attempt_at ?? "never"}</time></td>
            <td><code>{recentStatus ?? "none"}</code></td><td><time>{nextScheduledAt ?? "not scheduled"}</time></td>
            <td><button disabled={!connection.enabled} onClick={() => startManualSync(connection)} type="button">Manual Sync</button></td>
          </tr>)}
        </tbody></table></div>
      </section>
      <section className="connectors-section" aria-labelledby="coverage-state"><div><h2 id="coverage-state">Connector coverage</h2><span>Theoretical support, not Sync Coverage</span></div>
        {coverage.length === 0 ? <p className="connectors-empty">No connector coverage catalog is available.</p> : <div className="connectors-coverage">
          {coverage.map((item) => <article key={`${item.connector_type}-${item.module}`}><div><strong>{item.module}</strong><code>{item.connector_type}@{item.connector_version}</code></div><span className={`connectors-level level-${item.level}`}>{item.level}</span><p>{item.reason ?? "Declared support has no known gap."}</p></article>)}
        </div>}
      </section>
    </div>
  );
}
