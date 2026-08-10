import { Fragment, FormEvent, useCallback, useEffect, useState } from "react";

import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ConnectorCoverageDto } from "../../generated/query/ConnectorCoverageDto";
import type { SyncRunDto } from "../../generated/query/SyncRunDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import {
  desktopErrorCode,
  type ConnectionPurgeSummary,
  type GitHubRepositoryOption,
} from "../../platform/desktop-adapter/desktop-adapter";

import "./connectors.css";
import { SyncRunDetail } from "./SyncRunDetail";
import { usePersistedState } from "./use-persisted-state";

const CONNECTOR_DRAFT_PREFIX = "next-infra.connector-draft";

const SYNC_RUN_LIMIT = 5;

interface ConnectionRow {
  readonly connection: ConnectionDto;
  readonly nextScheduledAt: string | null;
  readonly recentStatus: string | null;
  readonly recentError: string | null;
  readonly recentWarning: string | null;
  readonly recentRuns: readonly SyncRunDto[];
}

interface PurgeConfirmation {
  readonly connection: ConnectionDto;
  readonly summary: ConnectionPurgeSummary;
}

interface ProviderSecretField {
  readonly key: string;
  readonly label: string;
}

interface ProviderFormDescriptor {
  readonly title: string;
  readonly subtitle: string;
  readonly nameLabel: string;
  readonly secretFields: readonly ProviderSecretField[];
  readonly regionLabel?: string;
  readonly defaultRegion?: string;
  readonly validateButtonLabel: string;
  readonly createButtonLabel: string;
  readonly validatedNotice: (count: number) => string;
  readonly createdNotice: (syncRunId: string) => string;
  readonly onValidate: (values: Record<string, string>) => Promise<number>;
  readonly onCreate: (values: Record<string, string>) => Promise<string>;
}

const PROVIDERS = [
  { id: "github", name: "GitHub", subtitle: "只读仓库、Actions 和部署数据" },
  { id: "ssh", name: "SSH", subtitle: "基于 SSH config 别名的只读探针" },
  { id: "dokploy", name: "Dokploy", subtitle: "只读项目、应用、部署、服务器与域名" },
  { id: "cloudflare", name: "Cloudflare", subtitle: "只读账户、区域、DNS 记录、隧道与 Worker" },
  { id: "supabase", name: "Supabase", subtitle: "只读组织与项目摘要（Management API）" },
  { id: "aliyun", name: "阿里云", subtitle: "只读 ECS、VPC、SLB、DNS 与公网 IP" },
  { id: "tencent", name: "腾讯云", subtitle: "只读 CVM、VPC、CLB、DNS 与公网 IP" },
];

const PROVIDER_TITLES: Record<string, string> = {
  github: "添加 GitHub 连接",
  ssh: "添加 SSH 连接",
  dokploy: "添加 Dokploy 连接",
  cloudflare: "添加 Cloudflare 连接",
  supabase: "添加 Supabase 连接",
  aliyun: "添加阿里云连接",
  tencent: "添加腾讯云连接",
};

function ProviderConnectionForm({
  descriptor,
  draftKey,
  onNotice,
  onError,
  onCreated,
}: {
  readonly descriptor: ProviderFormDescriptor;
  readonly draftKey: string;
  readonly onNotice: (text: string | null) => void;
  readonly onError: (text: string | null) => void;
  readonly onCreated: () => Promise<void>;
}) {
  const adapter = useDesktopAdapter();
  const [name, setName] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.${draftKey}.name`, "");
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [region, setRegion] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.${draftKey}.region`, descriptor.defaultRegion ?? "");
  const [validated, setValidated] = useState(false);
  const [validating, setValidating] = useState(false);
  const [connecting, setConnecting] = useState(false);

  const values = (): Record<string, string> => ({
    display_name: name,
    ...secrets,
    ...(descriptor.regionLabel !== undefined ? { region } : {}),
  });

  async function validateProvider() {
    if (validating || connecting) return;
    if (name.trim().length === 0) {
      onError("请先填写连接名称。");
      return;
    }
    for (const field of descriptor.secretFields) {
      if ((secrets[field.key] ?? "").trim().length === 0) {
        onError(`请先填写${field.label}。`);
        return;
      }
    }
    if (descriptor.regionLabel !== undefined && region.trim().length === 0) {
      onError(`请先填写${descriptor.regionLabel}。`);
      return;
    }
    onNotice(null);
    onError(null);
    setValidating(true);
    try {
      const count = await descriptor.onValidate(values());
      setValidated(true);
      onNotice(descriptor.validatedNotice(count));
    } catch (error) {
      setValidated(false);
      onError(`无法验证${descriptor.title}（${desktopErrorCode(error)}）。请检查配置后重试。`);
    } finally {
      setValidating(false);
    }
  }

  async function createProvider(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (validating || connecting) return;
    onNotice(null);
    onError(null);
    setConnecting(true);
    try {
      const syncRunId = await descriptor.onCreate(values());
      onNotice(descriptor.createdNotice(syncRunId));
      setName("");
      setSecrets({});
      setRegion(descriptor.defaultRegion ?? "");
      setValidated(false);
      await onCreated();
    } catch (error) {
      onError(`无法创建${descriptor.title}连接（${desktopErrorCode(error)}）。请检查配置后重试。`);
    } finally {
      setConnecting(false);
    }
  }

  return (
    <section className="connectors-section" aria-labelledby={`provider-${descriptor.title}`}>
      <form className="connectors-form" onSubmit={createProvider}>
        <label>{descriptor.nameLabel}<input autoComplete="off" disabled={validating || connecting} maxLength={120} onChange={(event) => { setName(event.target.value); setValidated(false); }} required value={name} /></label>
        {descriptor.secretFields.map((field) => (
          <label key={field.key}>{field.label}<input autoComplete="off" disabled={validating || connecting} maxLength={4096} onChange={(event) => { setSecrets((current) => ({ ...current, [field.key]: event.target.value })); setValidated(false); }} required type="password" value={secrets[field.key] ?? ""} /></label>
        ))}
        {descriptor.regionLabel !== undefined ? <label>{descriptor.regionLabel}<input autoComplete="off" disabled={validating || connecting} maxLength={64} onChange={(event) => { setRegion(event.target.value); setValidated(false); }} placeholder={descriptor.defaultRegion} required value={region} /></label> : null}
        <button disabled={validating || connecting} onClick={validateProvider} type="button">{validating ? "正在验证…" : descriptor.validateButtonLabel}{validating ? <span aria-hidden="true" className="connectors-button-progress" /> : null}</button>
        <button disabled={connecting || !validated} type="submit">{connecting ? "连接中…" : descriptor.createButtonLabel}</button>
      </form>
    </section>
  );
}

export function ConnectorsPage({ queryVersion = 0 }: { readonly queryVersion?: number }) {
  const adapter = useDesktopAdapter();
  const [rows, setRows] = useState<readonly ConnectionRow[] | null>(null);
  const [coverage, setCoverage] = useState<readonly ConnectorCoverageDto[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [displayName, setDisplayName] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.github.displayName`, "");
  const [token, setToken] = useState("");
  const [repositories, setRepositories] = usePersistedState<readonly GitHubRepositoryOption[] | null>(`${CONNECTOR_DRAFT_PREFIX}.github.repositories`, null);
  const [selectedRepositoryIds, setSelectedRepositoryIds] = usePersistedState<readonly string[]>(`${CONNECTOR_DRAFT_PREFIX}.github.selectedRepositoryIds`, []);
  const [discovering, setDiscovering] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [sshDisplayName, setSshDisplayName] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.ssh.displayName`, "");
  const [sshHostAlias, setSshHostAlias] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.ssh.hostAlias`, "");
  const [sshConnectTimeout, setSshConnectTimeout] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.ssh.connectTimeout`, "10");
  const [sshDiscoveredServices, setSshDiscoveredServices] = usePersistedState<readonly { id: string; name: string }[] | null>(`${CONNECTOR_DRAFT_PREFIX}.ssh.discoveredServices`, null);
  const [sshSelectedServiceIds, setSshSelectedServiceIds] = usePersistedState<readonly string[]>(`${CONNECTOR_DRAFT_PREFIX}.ssh.selectedServiceIds`, []);
  const [sshValidating, setSshValidating] = useState(false);
  const [sshConnecting, setSshConnecting] = useState(false);
  const [dokployDisplayName, setDokployDisplayName] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.dokploy.displayName`, "");
  const [dokployUrl, setDokployUrl] = usePersistedState(`${CONNECTOR_DRAFT_PREFIX}.dokploy.url`, "");
  const [dokployToken, setDokployToken] = useState("");
  const [dokployValidated, setDokployValidated] = useState(false);
  const [dokployValidating, setDokployValidating] = useState(false);
  const [dokployConnecting, setDokployConnecting] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const closeConnectorDialog = useCallback(() => {
    setDialogOpen(false);
    setToken("");
    setDokployToken("");
  }, []);
  const backToProviderPicker = useCallback(() => {
    setDialogProvider(null);
    setToken("");
    setDokployToken("");
  }, []);
  const [dialogProvider, setDialogProvider] = useState<string | null>(null);
  const [purgeConfirmation, setPurgeConfirmation] = useState<PurgeConfirmation | null>(null);
  const [purging, setPurging] = useState(false);
  const [expandedConnectionId, setExpandedConnectionId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const [connectionsSnapshot, coverageSnapshot] = await Promise.all([
      adapter.listConnections(),
      adapter.listConnectorCoverage(),
    ]);
    const statuses = await Promise.all(
      connectionsSnapshot.items.map(async (connection) => {
        const status = await adapter.getSyncStatus({ connection_id: connection.connection_id, recent_run_limit: SYNC_RUN_LIMIT });
        return {
          connection,
          nextScheduledAt: status.next_scheduled_at,
          recentRuns: status.recent_runs,
          recentStatus: status.recent_runs[0]?.status ?? null,
          recentError: status.recent_runs[0]?.errors[0]
            ? `${status.recent_runs[0].errors[0].code}: ${status.recent_runs[0].errors[0].message}`
            : null,
          recentWarning: status.recent_runs[0]?.warnings[0]
            ? `${status.recent_runs[0].warnings[0].code}: ${status.recent_runs[0].warnings[0].message}`
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
  }, [queryVersion, refresh]);

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
      setDialogOpen(false);
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

  async function discoverSshServices() {
    if (sshValidating || sshConnecting) return;
    if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(sshHostAlias.trim())) {
      setError("SSH 别名格式无效：以字母或数字开头，仅含字母/数字/._-，最长 128 字符。");
      return;
    }
    setNotice(null);
    setError(null);
    setSshValidating(true);
    try {
      const result = await adapter.validateSshConnection({
        host_alias: sshHostAlias.trim(),
        connect_timeout_secs: Number(sshConnectTimeout) || undefined,
      });
      setSshDiscoveredServices(result.discovered_services);
      setSshSelectedServiceIds([]);
      setNotice(
        result.discovered_services.length === 0
          ? "没有发现可用的服务，仍可创建空范围连接。"
          : `已发现 ${result.discovered_services.length} 个服务；请选择本次同步范围。`,
      );
    } catch (error) {
      setError(`无法验证 SSH 主机或发现服务（${desktopErrorCode(error)}）。请检查别名与连接配置后重试。`);
    } finally {
      setSshValidating(false);
    }
  }

  async function createSshConnection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (sshValidating || sshConnecting) return;
    setNotice(null);
    setError(null);
    setSshConnecting(true);
    try {
      const result = await adapter.createSshConnection({
        display_name: sshDisplayName,
        host_alias: sshHostAlias.trim(),
        connect_timeout_secs: Number(sshConnectTimeout) || undefined,
        allowed_service_ids: sshSelectedServiceIds,
      });
      setNotice(`SSH 连接已创建，将在后台同步 ${sshSelectedServiceIds.length} 个服务：${result.sync_run_id}。`);
      setDialogOpen(false);
      setSshDisplayName("");
      setSshHostAlias("");
      setSshConnectTimeout("10");
      setSshDiscoveredServices(null);
      setSshSelectedServiceIds([]);
      await refresh();
    } catch (error) {
      setError(`无法创建 SSH 连接（${desktopErrorCode(error)}）。请检查别名与连接配置后重试。`);
    } finally {
      setSshConnecting(false);
    }
  }

  function toggleSshService(serviceId: string) {
    setSshSelectedServiceIds((current) =>
      current.includes(serviceId)
        ? current.filter((id) => id !== serviceId)
        : [...current, serviceId],
    );
  }

  function isValidDokployUrl(value: string): boolean {
    try {
      const url = new URL(value.trim());
      return (url.protocol === "http:" || url.protocol === "https:") && url.hostname.length > 0;
    } catch {
      return false;
    }
  }

  async function validateDokployConnection() {
    if (dokployValidating || dokployConnecting) return;
    if (!isValidDokployUrl(dokployUrl)) {
      setError("Dokploy 实例 URL 无效：需以 http:// 或 https:// 开头且包含主机名。");
      return;
    }
    if (dokployToken.trim().length === 0) {
      setError("请先输入 Dokploy API Token。");
      return;
    }
    setNotice(null);
    setError(null);
    setDokployValidating(true);
    try {
      const result = await adapter.validateDokployConnection({
        url: dokployUrl.trim(),
        token: dokployToken,
      });
      setDokployValidated(true);
      setNotice(`已验证 Dokploy 实例，发现 ${result.project_count} 个项目。`);
    } catch (error) {
      setDokployValidated(false);
      setError(`无法验证 Dokploy 实例（${desktopErrorCode(error)}）。请检查 URL 与 Token 后重试。`);
    } finally {
      setDokployValidating(false);
    }
  }

  async function createDokployConnection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (dokployValidating || dokployConnecting) return;
    setNotice(null);
    setError(null);
    setDokployConnecting(true);
    try {
      const result = await adapter.createDokployConnection({
        display_name: dokployDisplayName,
        url: dokployUrl.trim(),
        token: dokployToken,
      });
      setNotice(`Dokploy 连接已创建，将在后台同步：${result.sync_run_id}。`);
      setDialogOpen(false);
      setDokployDisplayName("");
      setDokployUrl("");
      setDokployToken("");
      setDokployValidated(false);
      await refresh();
    } catch (error) {
      setError(`无法创建 Dokploy 连接（${desktopErrorCode(error)}）。请检查 URL 与 Token 后重试。`);
    } finally {
      setDokployConnecting(false);
    }
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
            <section className="connectors-section" aria-labelledby="add-connection">
        <div className="overview-section-heading">
          <div>
            <p className="connectors-eyebrow">连接器配置</p>
            <h2 id="add-connection">添加连接</h2>
            <p>按凭据模型逐项录入，验证通过后建立只读连接。</p>
          </div>
          <button type="button" onClick={() => { setDialogOpen(true); setDialogProvider(null); }}>添加连接</button>
        </div>
      </section>
      {dialogOpen ? (
        <div className="connectors-dialog-overlay" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) { closeConnectorDialog(); } }}>
          <div className="connectors-dialog" role="dialog" aria-modal="true" aria-label="添加连接">
            <div className="connectors-dialog-header">
              <h2>{dialogProvider === null ? "选择连接器" : PROVIDER_TITLES[dialogProvider]}</h2>
              <div className="connectors-dialog-actions">
                {dialogProvider !== null ? <button onClick={backToProviderPicker} type="button">返回</button> : null}
                <button onClick={closeConnectorDialog} type="button">关闭</button>
              </div>
            </div>
            {dialogProvider === null ? (
              <div className="connectors-provider-grid">
                {PROVIDERS.map((provider) => (
                  <button key={provider.id} className="connectors-provider-option" onClick={() => setDialogProvider(provider.id)} type="button">
                    <strong>{provider.name}</strong><span>{provider.subtitle}</span>
                  </button>
                ))}
              </div>
            ) : (
              <>
{dialogProvider === "github" ? (<section className="connectors-section" aria-labelledby="github-connection">
        <form className="connectors-form" onSubmit={createGitHubConnection}>
          <label>连接名称<input autoComplete="off" disabled={connecting || discovering} maxLength={120} onChange={(event) => setDisplayName(event.target.value)} required value={displayName} /></label>
          <label>细粒度 Token<input autoComplete="off" disabled={connecting || discovering} maxLength={16384} onChange={(event) => setToken(event.target.value)} required type="password" value={token} /></label>
          <button disabled={connecting || discovering} onClick={discoverGitHubRepositories} type="button">{discovering ? "正在加载…" : "验证并加载仓库"}{discovering ? <span aria-hidden="true" className="connectors-button-progress" /> : null}</button>
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
      ) : null}{dialogProvider === "ssh" ? (<section className="connectors-section" aria-labelledby="ssh-connection">
        <form className="connectors-form" onSubmit={createSshConnection}>
          <label>SSH 连接名称<input autoComplete="off" disabled={sshValidating || sshConnecting} maxLength={120} onChange={(event) => setSshDisplayName(event.target.value)} required value={sshDisplayName} /></label>
          <label>主机别名<input autoComplete="off" disabled={sshValidating || sshConnecting} maxLength={128} onChange={(event) => setSshHostAlias(event.target.value)} placeholder="例如 mac-mini" required value={sshHostAlias} /><span className="connectors-field-hint">SSH config 中的别名：字母/数字开头，仅含字母、数字、_ . -</span></label>
          <label>连接超时（秒）<input autoComplete="off" disabled={sshValidating || sshConnecting} maxLength={4} onChange={(event) => setSshConnectTimeout(event.target.value)} type="number" value={sshConnectTimeout} /></label>
          <button disabled={sshValidating || sshConnecting} onClick={discoverSshServices} type="button">{sshValidating ? "正在验证…" : "验证并发现服务"}{sshValidating ? <span aria-hidden="true" className="connectors-button-progress" /> : null}</button>
          {sshDiscoveredServices !== null ? <fieldset className="connectors-repository-picker">
            <legend>同步范围：已选择 {sshSelectedServiceIds.length} / {sshDiscoveredServices.length} 个服务</legend>
            {sshDiscoveredServices.length === 0 ? <p>没有可用的服务，仍可创建空范围连接。</p> : <div className="connectors-repository-list">
              {sshDiscoveredServices.map((service) => <label key={service.id}>
                <input checked={sshSelectedServiceIds.includes(service.id)} disabled={sshConnecting} onChange={() => toggleSshService(service.id)} type="checkbox" />
                <span>{service.name}</span>
              </label>)}
            </div>}
            <button disabled={sshConnecting} type="submit">{sshConnecting ? "连接中…" : `创建连接并同步 ${sshSelectedServiceIds.length} 个服务`}</button>
          </fieldset> : null}
        </form>
      </section>
      ) : null}{dialogProvider === "dokploy" ? (<section className="connectors-section" aria-labelledby="dokploy-connection">
        <form className="connectors-form" onSubmit={createDokployConnection}>
          <label>Dokploy 连接名称<input autoComplete="off" disabled={dokployValidating || dokployConnecting} maxLength={120} onChange={(event) => { setDokployDisplayName(event.target.value); setDokployValidated(false); }} required value={dokployDisplayName} /></label>
          <label>实例 URL<input autoComplete="off" disabled={dokployValidating || dokployConnecting} maxLength={512} onChange={(event) => { setDokployUrl(event.target.value); setDokployValidated(false); }} placeholder="https://dokploy.example.com" required value={dokployUrl} /><span className="connectors-field-hint">实例基础地址（不带 /api 后缀）</span></label>
          <label>API Token<input autoComplete="off" disabled={dokployValidating || dokployConnecting} maxLength={4096} onChange={(event) => { setDokployToken(event.target.value); setDokployValidated(false); }} required type="password" value={dokployToken} /></label>
          <button disabled={dokployValidating || dokployConnecting} onClick={validateDokployConnection} type="button">{dokployValidating ? "正在验证…" : "验证并统计项目"}{dokployValidating ? <span aria-hidden="true" className="connectors-button-progress" /> : null}</button>
          <button disabled={dokployConnecting || !dokployValidated} type="submit">{dokployConnecting ? "连接中…" : "创建连接并同步"}</button>
        </form>
      </section>
      ) : null}      {dialogProvider === "cloudflare" ? (
      <ProviderConnectionForm
        descriptor={{
          title: "添加 Cloudflare 连接",
          subtitle: "只读账户、区域、DNS 记录、隧道与 Worker 摘要",
          nameLabel: "Cloudflare 连接名称",
          secretFields: [{ key: "token", label: "Cloudflare Token" }],
          validateButtonLabel: "验证并统计账户",
          createButtonLabel: "创建 Cloudflare 连接并同步",
          validatedNotice: (count) => `已验证 Cloudflare 账户，发现 ${count} 个账户。`,
          createdNotice: (syncRunId) => `Cloudflare 连接已创建，将在后台同步：${syncRunId}。`,
          onValidate: async (v) => (await adapter.validateCloudflareConnection({ token: v.token })).account_count,
          onCreate: async (v) => (await adapter.createCloudflareConnection({ display_name: v.display_name, token: v.token })).sync_run_id,
        }}
        onNotice={setNotice}
        onError={setError}
        draftKey="cloudflare"
        onCreated={async () => { setDialogOpen(false); await refresh(); }}
      />
      ) : null}
      {dialogProvider === "supabase" ? (
      <ProviderConnectionForm
        descriptor={{
          title: "添加 Supabase 连接",
          subtitle: "只读组织与项目摘要（Management API）",
          nameLabel: "Supabase 连接名称",
          secretFields: [{ key: "token", label: "Supabase Token" }],
          validateButtonLabel: "验证并统计 Supabase 项目",
          createButtonLabel: "创建 Supabase 连接并同步",
          validatedNotice: (count) => `已验证 Supabase 账户，发现 ${count} 个项目。`,
          createdNotice: (syncRunId) => `Supabase 连接已创建，将在后台同步：${syncRunId}。`,
          onValidate: async (v) => (await adapter.validateSupabaseManagedConnection({ token: v.token })).project_count,
          onCreate: async (v) => (await adapter.createSupabaseManagedConnection({ display_name: v.display_name, token: v.token })).sync_run_id,
        }}
        onNotice={setNotice}
        onError={setError}
        draftKey="supabase"
        onCreated={async () => { setDialogOpen(false); await refresh(); }}
      />
      ) : null}
      {dialogProvider === "aliyun" ? (
      <ProviderConnectionForm
        descriptor={{
          title: "添加阿里云连接",
          subtitle: "只读 ECS、VPC、SLB、DNS 与公网 IP 摘要",
          nameLabel: "阿里云连接名称",
          secretFields: [
            { key: "access_key_id", label: "阿里云 AccessKey ID" },
            { key: "access_key_secret", label: "阿里云 AccessKey Secret" },
          ],
          regionLabel: "区域",
          defaultRegion: "cn-hangzhou",
          validateButtonLabel: "验证并统计阿里云资源",
          createButtonLabel: "创建阿里云连接并同步",
          validatedNotice: (count) => `已验证阿里云凭据，发现 ${count} 个资源。`,
          createdNotice: (syncRunId) => `阿里云连接已创建，将在后台同步：${syncRunId}。`,
          onValidate: async (v) => (await adapter.validateAliyunConnection({ access_key_id: v.access_key_id, access_key_secret: v.access_key_secret, region: v.region })).resource_count,
          onCreate: async (v) => (await adapter.createAliyunConnection({ display_name: v.display_name, access_key_id: v.access_key_id, access_key_secret: v.access_key_secret, region: v.region })).sync_run_id,
        }}
        onNotice={setNotice}
        onError={setError}
        draftKey="aliyun"
        onCreated={async () => { setDialogOpen(false); await refresh(); }}
      />
      ) : null}
      {dialogProvider === "tencent" ? (
      <ProviderConnectionForm
        descriptor={{
          title: "添加腾讯云连接",
          subtitle: "只读 CVM、VPC、CLB、DNS 与公网 IP 摘要",
          nameLabel: "腾讯云连接名称",
          secretFields: [
            { key: "secret_id", label: "腾讯云 SecretId" },
            { key: "secret_key", label: "腾讯云 SecretKey" },
          ],
          regionLabel: "区域",
          defaultRegion: "ap-guangzhou",
          validateButtonLabel: "验证并统计腾讯云资源",
          createButtonLabel: "创建腾讯云连接并同步",
          validatedNotice: (count) => `已验证腾讯云凭据，发现 ${count} 个资源。`,
          createdNotice: (syncRunId) => `腾讯云连接已创建，将在后台同步：${syncRunId}。`,
          onValidate: async (v) => (await adapter.validateTencentConnection({ secret_id: v.secret_id, secret_key: v.secret_key, region: v.region })).resource_count,
          onCreate: async (v) => (await adapter.createTencentConnection({ display_name: v.display_name, secret_id: v.secret_id, secret_key: v.secret_key, region: v.region })).sync_run_id,
        }}
        onNotice={setNotice}
        onError={setError}
        draftKey="tencent"
        onCreated={async () => { setDialogOpen(false); await refresh(); }}
      />
      ) : null}
              </>
            )}
          </div>
        </div>
      ) : null}
<section className="connectors-section" aria-labelledby="connection-state"><div><h2 id="connection-state">连接状态</h2><span>手动同步与页面刷新相互独立</span></div>
        <div className="connectors-frame"><table><thead><tr><th>连接</th><th>健康度</th><th>最近成功</th><th>最近尝试</th><th>最近运行</th><th>最近错误</th><th>最近警告</th><th>下次计划</th><th>操作</th></tr></thead><tbody>
          {rows.map((row) => {
            const { connection, nextScheduledAt, recentStatus, recentError, recentWarning } = row;
            const expanded = expandedConnectionId === connection.connection_id;
            return <Fragment key={connection.connection_id}>
            <tr>
            <td><button aria-controls={`connectors-run-detail-${connection.connection_id}`} aria-expanded={expanded} className="connectors-expand" onClick={() => setExpandedConnectionId(expanded ? null : connection.connection_id)} type="button">{expanded ? "收起" : "展开"}</button><strong>{connection.display_name}</strong><code>{connection.connector_type}</code></td>
            <td><span className={`connectors-status state-${connection.health}`}>{displayEnum(connection.health)}</span></td>
            <td><time>{connection.last_success_at ?? "从未"}</time></td><td><time>{connection.last_attempt_at ?? "从未"}</time></td>
            <td><code>{recentStatus ? displayEnum(recentStatus) : "无"}</code></td><td><span>{recentError ?? "无"}</span></td><td><span>{recentWarning ?? "无"}</span></td><td><time>{nextScheduledAt ?? "未计划"}</time></td>
            <td><div className="connectors-actions"><button disabled={!connection.enabled || connection.connector_type !== "github" || connecting || purging} onClick={() => startManualSync(connection)} type="button">手动同步</button><button disabled={connection.connector_type !== "github" || connecting || purging} onClick={() => previewGitHubConnectionPurge(connection)} type="button">删除本地数据</button></div></td>
            </tr>
            {expanded ? <tr className="connectors-detail-row" id={`connectors-run-detail-${connection.connection_id}`}><td colSpan={9}><SyncRunDetail connection={connection} runs={row.recentRuns} /></td></tr> : null}
            </Fragment>;
          })}
        </tbody></table></div>
        {rows.length === 0 ? <p className="connectors-empty connectors-setup-hint">尚无本地连接。请在「添加连接」中配置第一个只读连接器，建立首次同步。</p> : null}
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
