import { FormEvent, useCallback, useEffect, useState } from "react";

import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ConnectorCoverageDto } from "../../generated/query/ConnectorCoverageDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import {
  desktopErrorCode,
  type ConnectionPurgeSummary,
  type GitHubRepositoryOption,
} from "../../platform/desktop-adapter/desktop-adapter";

import "./connectors.css";

interface ConnectionRow {
  readonly connection: ConnectionDto;
  readonly nextScheduledAt: string | null;
  readonly recentStatus: string | null;
  readonly recentError: string | null;
}

interface PurgeConfirmation {
  readonly connection: ConnectionDto;
  readonly summary: ConnectionPurgeSummary;
}

export function ConnectorsPage() {
  const adapter = useDesktopAdapter();
  const [rows, setRows] = useState<readonly ConnectionRow[] | null>(null);
  const [coverage, setCoverage] = useState<readonly ConnectorCoverageDto[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [displayName, setDisplayName] = useState("");
  const [token, setToken] = useState("");
  const [repositories, setRepositories] = useState<readonly GitHubRepositoryOption[] | null>(null);
  const [selectedRepositoryIds, setSelectedRepositoryIds] = useState<readonly string[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [purgeConfirmation, setPurgeConfirmation] = useState<PurgeConfirmation | null>(null);
  const [purging, setPurging] = useState(false);

  const refresh = useCallback(async () => {
    const [connectionsSnapshot, coverageSnapshot] = await Promise.all([
      adapter.listConnections(),
      adapter.listConnectorCoverage(),
    ]);
    const statuses = await Promise.all(
      connectionsSnapshot.items.map(async (connection) => {
        const status = await adapter.getSyncStatus({ connection_id: connection.connection_id, recent_run_limit: 1 });
        return {
          connection,
          nextScheduledAt: status.next_scheduled_at,
          recentStatus: status.recent_runs[0]?.status ?? null,
          recentError: status.recent_runs[0]?.errors[0]
            ? `${status.recent_runs[0].errors[0].code}: ${status.recent_runs[0].errors[0].message}`
            : null,
        };
      }),
    );
    setRows(statuses);
    setCoverage(coverageSnapshot.items);
  }, [adapter]);

  useEffect(() => {
    let active = true;
    refresh()
      .then(() => { if (active) setError(null); })
      .catch((error) => { if (active) setError(`无法查询连接器状态（${desktopErrorCode(error)}）。`); });
    return () => { active = false; };
  }, [refresh]);

  async function createGitHubConnection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (connecting || discovering) return;
    if (selectedRepositoryIds.length === 0) {
      setError("请至少选择一个 GitHub 仓库后再创建连接。");
      return;
    }
    setNotice(null);
    setError(null);
    setConnecting(true);
    try {
      const result = await adapter.createGitHubConnection({
        display_name: displayName,
        token,
        selected_repository_ids: selectedRepositoryIds,
      });
      setNotice(`GitHub 连接已创建，将在后台同步 ${selectedRepositoryIds.length} 个选定仓库：${result.sync_run_id}。`);
      setDisplayName("");
      setRepositories(null);
      setSelectedRepositoryIds([]);
      await refresh();
    } catch (error) {
      setError(`无法创建 GitHub 连接（${desktopErrorCode(error)}）。请检查 Token 权限后重试。`);
    } finally {
      setToken("");
      setConnecting(false);
    }
  }

  async function discoverGitHubRepositories() {
    if (discovering || connecting) return;
    if (token.trim().length === 0) {
      setError("请先输入 GitHub Token。");
      return;
    }
    setNotice(null);
    setError(null);
    setDiscovering(true);
    try {
      const discovered = await adapter.discoverGitHubRepositories(token);
      setRepositories(discovered);
      setSelectedRepositoryIds([]);
      setNotice(
        discovered.length === 0
          ? "没有发现可访问的 GitHub 仓库。"
          : `已加载 ${discovered.length} 个可访问仓库；请选择本次同步范围。`,
      );
    } catch (error) {
      setError(`无法验证 Token 或加载 GitHub 仓库（${desktopErrorCode(error)}）。请检查 Token 权限或网络后重试。`);
    } finally {
      setDiscovering(false);
    }
  }

  function toggleRepository(repositoryId: string) {
    setSelectedRepositoryIds((current) =>
      current.includes(repositoryId)
        ? current.filter((id) => id !== repositoryId)
        : [...current, repositoryId],
    );
  }

  async function startManualSync(connection: ConnectionDto) {
    setNotice(null);
    try {
      const result = await adapter.manualSync(connection.connection_id);
      setNotice(`已将手动同步加入队列：${result.sync_run_id}。当前页面未刷新。`);
    } catch {
      setNotice("无法加入手动同步队列，现有快照事实未改变。");
    }
  }

  async function previewGitHubConnectionPurge(connection: ConnectionDto) {
    if (purging) return;
    setNotice(null);
    setError(null);
    try {
      const summary = await adapter.previewGitHubConnectionPurge(connection.connection_id);
      setPurgeConfirmation({ connection, summary });
    } catch {
      setError("无法读取该连接的本地快照清理范围。");
    }
  }

  async function purgeGitHubConnection() {
    if (purgeConfirmation === null || purging) return;
    setPurging(true);
    setNotice(null);
    setError(null);
    try {
      const summary = await adapter.purgeGitHubConnection(
        purgeConfirmation.connection.connection_id,
      );
      setPurgeConfirmation(null);
      setNotice(
        `已删除 ${summary.resources} 个资源、${summary.relations} 条关系和 ${summary.changes} 项变更。`,
      );
      await refresh();
    } catch {
      setError("无法删除该连接的本地快照；现有数据已保留。");
    } finally {
      setPurging(false);
    }
  }

  if (error !== null && rows === null) return <section className="connectors-state connectors-state--error" role="alert">{error}</section>;
  if (rows === null) return <section className="connectors-state" aria-busy="true">正在读取连接器状态…</section>;

  return (
    <div className="connectors-page">
      <header><div><p className="connectors-eyebrow">观测传输</p><h1>连接器</h1><p>核验只读健康度与声明覆盖范围，不要将它们与资源健康度混为一谈。</p></div><span>{rows.length} 个本地连接</span></header>
      {notice ? <div className="connectors-notice" role="status">{notice}</div> : null}
      {error ? <div className="connectors-notice connectors-notice--error" role="alert">{error}</div> : null}
      {purgeConfirmation ? <section className="connectors-purge-confirmation" role="alert">
        <div><h2>删除本地快照</h2><p>将永久删除“{purgeConfirmation.connection.display_name}”的本地数据与凭据，无法恢复。</p></div>
        <dl>
          <div><dt>资源</dt><dd>{purgeConfirmation.summary.resources}</dd></div>
          <div><dt>关系</dt><dd>{purgeConfirmation.summary.relations}</dd></div>
          <div><dt>资源版本</dt><dd>{purgeConfirmation.summary.resource_versions}</dd></div>
          <div><dt>关系版本</dt><dd>{purgeConfirmation.summary.relation_versions}</dd></div>
          <div><dt>变更</dt><dd>{purgeConfirmation.summary.changes}</dd></div>
          <div><dt>绑定</dt><dd>{purgeConfirmation.summary.bindings}</dd></div>
          <div><dt>同步记录</dt><dd>{purgeConfirmation.summary.sync_runs}</dd></div>
        </dl>
        {purgeConfirmation.summary.bindings > 0 ? <p>包含关联到该连接资源的手工绑定；这些绑定也会被删除。</p> : null}
        <div className="connectors-purge-actions"><button disabled={purging} onClick={() => setPurgeConfirmation(null)} type="button">取消</button><button disabled={purging} onClick={purgeGitHubConnection} type="button">{purging ? "正在删除…" : "确认删除本地快照"}</button></div>
      </section> : null}
      <section className="connectors-section" aria-labelledby="github-connection">
        <div><h2 id="github-connection">添加 GitHub 连接</h2><span>只读仓库、Actions 和部署数据</span></div>
        <form className="connectors-form" onSubmit={createGitHubConnection}>
          <label>连接名称<input autoComplete="off" disabled={connecting || discovering} maxLength={120} onChange={(event) => setDisplayName(event.target.value)} required value={displayName} /></label>
          <label>细粒度 Token<input autoComplete="off" disabled={connecting || discovering} maxLength={16384} onChange={(event) => setToken(event.target.value)} required type="password" value={token} /></label>
          <button disabled={connecting || discovering} onClick={discoverGitHubRepositories} type="button">{discovering ? "正在加载…" : "验证并加载仓库"}</button>
          {repositories !== null ? <fieldset className="connectors-repository-picker">
            <legend>同步范围：已选择 {selectedRepositoryIds.length} / {repositories.length} 个仓库</legend>
            {repositories.length === 0 ? <p>该 Token 没有可同步的仓库。</p> : <div className="connectors-repository-list">
              {repositories.map((repository) => <label key={repository.id}>
                <input checked={selectedRepositoryIds.includes(repository.id)} disabled={connecting} onChange={() => toggleRepository(repository.id)} type="checkbox" />
                <span>{repository.name}</span>
              </label>)}
            </div>}
            <button disabled={connecting || selectedRepositoryIds.length === 0} type="submit">{connecting ? "连接中…" : `创建连接并同步 ${selectedRepositoryIds.length} 个仓库`}</button>
          </fieldset> : null}
        </form>
      </section>
      <section className="connectors-section" aria-labelledby="connection-state"><div><h2 id="connection-state">连接状态</h2><span>手动同步与页面刷新相互独立</span></div>
        <div className="connectors-frame"><table><thead><tr><th>连接</th><th>健康度</th><th>最近成功</th><th>最近尝试</th><th>最近运行</th><th>最近错误</th><th>下次计划</th><th>操作</th></tr></thead><tbody>
          {rows.map(({ connection, nextScheduledAt, recentStatus, recentError }) => <tr key={connection.connection_id}>
            <td><strong>{connection.display_name}</strong><code>{connection.connector_type}</code></td>
            <td><span className={`connectors-status state-${connection.health}`}>{displayEnum(connection.health)}</span></td>
            <td><time>{connection.last_success_at ?? "从未"}</time></td><td><time>{connection.last_attempt_at ?? "从未"}</time></td>
            <td><code>{recentStatus ? displayEnum(recentStatus) : "无"}</code></td><td><span>{recentError ?? "无"}</span></td><td><time>{nextScheduledAt ?? "未计划"}</time></td>
            <td><div className="connectors-actions"><button disabled={!connection.enabled || connection.connector_type !== "github" || connecting || purging} onClick={() => startManualSync(connection)} type="button">手动同步</button><button disabled={connection.connector_type !== "github" || connecting || purging} onClick={() => previewGitHubConnectionPurge(connection)} type="button">删除本地数据</button></div></td>
          </tr>)}
        </tbody></table></div>
      </section>
      <section className="connectors-section" aria-labelledby="coverage-state"><div><h2 id="coverage-state">连接器覆盖矩阵</h2><span>声明的模块范围，不等于同步覆盖或连接健康度</span></div>
        {coverage.length === 0 ? <p className="connectors-empty">没有可用的连接器覆盖目录。</p> : <div className="connectors-coverage">
          <div className="connectors-coverage-head" role="row"><span>连接器</span><span>模块</span><span>声明级别</span><span>边界 / 缺口</span></div>
          {coverage.map((item) => <article key={`${item.connector_type}-${item.module}`}><div><strong>{item.connector_type}</strong><code>v{item.connector_version}</code></div><strong>{item.module}</strong><span className={`connectors-level level-${item.level}`}>{displayEnum(item.level)}</span><p>{item.reason ?? "声明支持范围没有已知缺口。"}</p></article>)}
        </div>}
      </section>
    </div>
  );
}
