import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { SyncCoverageDto } from "../../generated/query/SyncCoverageDto";
import type { SyncRunDto } from "../../generated/query/SyncRunDto";
import type { SyncRunStatusDto } from "../../generated/query/SyncRunStatusDto";
import { displayEnum } from "../../i18n";

const statusTone: Readonly<Record<SyncRunStatusDto, "green" | "amber" | "vermilion" | "cloud">> = {
  running: "amber",
  succeeded: "green",
  partial: "amber",
  failed: "vermilion",
  cancelled: "cloud",
  interrupted: "amber",
};

const coverageTone: Readonly<Record<SyncCoverageDto["type"], "green" | "amber" | "cloud">> = {
  authoritative_full: "green",
  incremental: "amber",
  partial: "amber",
  targeted: "cloud",
};

function coverageSummary(coverage: SyncCoverageDto): string {
  switch (coverage.type) {
    case "authoritative_full":
      return `权威完整 · ${coverage.scope}`;
    case "incremental":
      return `增量 · cursor ${coverage.cursor}`;
    case "partial":
      return `部分覆盖${coverage.scope === null ? "" : ` · ${coverage.scope}`}`;
    case "targeted":
      return `定向 · ${coverage.resource_ids.length} 个资源`;
  }
}

function CoverageFacts({ coverage }: { readonly coverage: SyncCoverageDto }) {
  switch (coverage.type) {
    case "authoritative_full":
      return <code>{coverage.scope}</code>;
    case "incremental":
      return <code>cursor {coverage.cursor}</code>;
    case "partial":
      return (
        <span className="connectors-run-coverage-partial">
          <code>{coverage.scope ?? "未声明范围"}</code>
          {coverage.reason ? <span className="connectors-run-reason">{coverage.reason}</span> : null}
        </span>
      );
    case "targeted":
      return <code>{coverage.resource_ids.length} 个资源</code>;
  }
}

function RunMessages({ run }: { readonly run: SyncRunDto }) {
  if (run.errors.length === 0 && run.warnings.length === 0) return null;
  return (
    <ul className="connectors-run-message-list">
      {run.errors.map((error) => (
        <li key={error.code} className="connectors-run-message connectors-run-message--error">
          {error.code}: {error.message}
          {error.retryable ? "（可重试）" : ""}
        </li>
      ))}
      {run.warnings.map((warning) => (
        <li key={warning.code} className="connectors-run-message">
          {warning.code}: {warning.message}
        </li>
      ))}
    </ul>
  );
}

function SyncRunCard({ run }: { readonly run: SyncRunDto }) {
  return (
    <article className="connectors-run" aria-label={`同步记录 ${run.sync_run_id}`}>
      <header className="connectors-run-head">
        <span className={`connectors-chip connectors-chip--${statusTone[run.status]}`}>
          {displayEnum(run.status)}
        </span>
        <code>{run.sync_run_id}</code>
        <time className="connectors-run-time" dateTime={run.started_at}>
          {run.started_at} → {run.finished_at ?? "进行中"}
        </time>
      </header>
      <dl className="connectors-run-facts">
        <div><dt>模式</dt><dd>{displayEnum(run.mode)}</dd></div>
        <div><dt>触发</dt><dd>{displayEnum(run.trigger)}</dd></div>
        <div><dt>覆盖类型</dt><dd><span className={`connectors-chip connectors-chip--${coverageTone[run.coverage.type]}`}>{displayEnum(run.coverage.type)}</span></dd></div>
        <div><dt>覆盖详情</dt><dd><CoverageFacts coverage={run.coverage} /></dd></div>
        <div><dt>开始时间</dt><dd><time dateTime={run.started_at}>{run.started_at}</time></dd></div>
        <div><dt>结束时间</dt><dd>{run.finished_at === null ? <span>进行中</span> : <time dateTime={run.finished_at}>{run.finished_at}</time>}</dd></div>
        <div><dt>读取 / 创建 / 更新 / 未变 / 警告</dt><dd><code>读取 {run.counts.read} · 创建 {run.counts.created} · 更新 {run.counts.updated} · 未变 {run.counts.unchanged} · 警告 {run.counts.warnings}</code></dd></div>
      </dl>
      <RunMessages run={run} />
    </article>
  );
}

function ProvenanceChain({
  connection,
  latest,
}: {
  readonly connection: ConnectionDto;
  readonly latest: SyncRunDto | undefined;
}) {
  return (
    <ol className="connectors-provenance" aria-label="同步来源链">
      <li className="connectors-provenance-step">
        <span className="connectors-provenance-label">连接器</span>
        <code className="connectors-provenance-value">{connection.connector_type}</code>
        <span className="connectors-provenance-meta">编译的只读适配器</span>
      </li>
      <li className="connectors-provenance-step">
        <span className="connectors-provenance-label">连接</span>
        <span className="connectors-provenance-value">{connection.display_name}</span>
        <span className="connectors-provenance-meta">凭据仅存本机</span>
      </li>
      <li className="connectors-provenance-step">
        <span className="connectors-provenance-label">同步运行</span>
        {latest === undefined ? (
          <span className="connectors-provenance-value connectors-provenance-value--empty">从未成功同步</span>
        ) : (
          <code className="connectors-provenance-value">{latest.sync_run_id}</code>
        )}
        <span className="connectors-provenance-meta">
          {latest === undefined ? "尚无 SyncRun 记录" : `${displayEnum(latest.status)} · ${latest.started_at}`}
        </span>
      </li>
      <li className="connectors-provenance-step connectors-provenance-step--active">
        <span className="connectors-provenance-label">覆盖</span>
        {latest === undefined ? (
          <span className="connectors-provenance-value connectors-provenance-value--empty">无覆盖记录</span>
        ) : (
          <span className="connectors-provenance-value">{coverageSummary(latest.coverage)}</span>
        )}
        <span className="connectors-provenance-meta">
          {latest === undefined ? "等待首次同步" : displayEnum(latest.mode)}
        </span>
      </li>
    </ol>
  );
}

export function SyncRunDetail({
  connection,
  runs,
}: {
  readonly connection: ConnectionDto;
  readonly runs: readonly SyncRunDto[];
}) {
  return (
    <div className="connectors-run-detail">
      <ProvenanceChain connection={connection} latest={runs[0]} />
      {runs.length === 0 ? (
        <p className="connectors-sync-empty">
          该连接从未成功同步。尚无 SyncRun 记录；手动同步或等待下次计划后重试。
        </p>
      ) : (
        <div className="connectors-runs">
          {runs.map((run) => <SyncRunCard key={run.sync_run_id} run={run} />)}
        </div>
      )}
    </div>
  );
}
