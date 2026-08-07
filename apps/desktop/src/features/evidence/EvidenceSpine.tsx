import { useId } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";

import "./EvidenceSpine.css";

interface EvidenceSpineProps {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

const evidenceLabels = {
  provider: "提供方",
  configured: "已配置",
  inferred: "推断",
} as const;

function CurrentFact({
  label,
  resource,
}: {
  readonly label: string;
  readonly resource: ResourceDto;
}) {
  return (
    <article className="evidence-spine__fact" aria-label={`${label} 当前事实`}>
      <span className="evidence-spine__eyebrow">{label}</span>
      <strong>{resource.display_name}</strong>
      <code>{resource.resource_id}</code>
      <div className="evidence-spine__status-line">
        <span><small>健康度</small>{displayEnum(resource.health)}</span>
        <span><small>新鲜度</small>{displayEnum(resource.freshness)}</span>
        <span><small>生命周期</small>{displayEnum(resource.lifecycle)}</span>
      </div>
      <time dateTime={resource.observed_at}>{resource.observed_at}</time>
    </article>
  );
}

function Confidence({ basisPoints }: { readonly basisPoints: number }) {
  const percentage = basisPoints / 100;
  return (
    <span>
      {percentage}% <small>({basisPoints} bp)</small>
    </span>
  );
}

function EvidenceDetails({ relation }: { readonly relation: RelationDto }) {
  const evidence = relation.evidence;

  if (evidence.type === "provider") {
    return (
      <dl className="evidence-spine__details">
        <dt>提供方</dt>
        <dd><code>{evidence.connector_type}</code></dd>
        <dt>连接</dt>
        <dd><code>{evidence.connection_id}</code></dd>
        <dt>SyncRun</dt>
        <dd><code>{evidence.sync_run_id}</code></dd>
        <dt>字段路径</dt>
        <dd><code>{evidence.field_path}</code></dd>
        <dt>最近观测</dt>
        <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
      </dl>
    );
  }

  if (evidence.type === "configured") {
    return (
      <dl className="evidence-spine__details">
        <dt>绑定</dt>
        <dd><code>{evidence.binding_id}</code></dd>
        <dt>创建时间</dt>
        <dd><time dateTime={evidence.created_at}>{evidence.created_at}</time></dd>
        <dt>最近观测</dt>
        <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
      </dl>
    );
  }

  return (
    <dl className="evidence-spine__details">
        <dt>规则版本</dt>
      <dd><code>{evidence.rule_version}</code></dd>
        <dt>输入版本</dt>
      <dd>
        <ul className="evidence-spine__inputs" aria-label="输入资源版本">
          {evidence.input_resource_version_ids.map((versionId) => (
            <li key={versionId}><code>{versionId}</code></li>
          ))}
        </ul>
      </dd>
        <dt>关系输入</dt>
      <dd>
        {evidence.input_relation_version_ids.length === 0 ? (
          <span>无</span>
        ) : (
          <ul className="evidence-spine__inputs" aria-label="输入关系版本">
            {evidence.input_relation_version_ids.map((versionId) => (
              <li key={versionId}><code>{versionId}</code></li>
            ))}
          </ul>
        )}
      </dd>
        <dt>置信度</dt>
      <dd><Confidence basisPoints={evidence.confidence_basis_points} /></dd>
        <dt>最近观测</dt>
      <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
    </dl>
  );
}

export function EvidenceSpine({ source, target, relations }: EvidenceSpineProps) {
  const titleId = useId();
  const factsId = useId();
  const pathId = useId();

  return (
    <section className="evidence-spine" aria-labelledby={titleId}>
      <header className="evidence-spine__header">
        <div>
          <span className="evidence-spine__eyebrow">检查器</span>
          <h2 id={titleId}>证据链</h2>
        </div>
        <span className="evidence-spine__count">
          {relations.length} 个来源
        </span>
      </header>

      <section className="evidence-spine__section" aria-labelledby={factsId}>
        <h3 id={factsId}>当前事实</h3>
        <div className="evidence-spine__facts">
          <CurrentFact label="来源" resource={source} />
          <CurrentFact label="目标" resource={target} />
        </div>
      </section>

      <section className="evidence-spine__section" aria-labelledby={pathId}>
        <h3 id={pathId}>证据路径</h3>
        {relations.length === 0 ? (
          <p className="evidence-spine__empty">这些端点没有可用证据。</p>
        ) : (
          <ol className="evidence-spine__path">
            {relations.map((relation) => {
              const type = relation.evidence.type;
              return (
                <li
                  className={`evidence-spine__evidence evidence-spine__evidence--${type}`}
                  key={relation.relation_id}
                  aria-label={`${evidenceLabels[type]} evidence`}
                >
                  <div className="evidence-spine__evidence-header">
                    <span className="evidence-spine__type">{evidenceLabels[type]}</span>
                    <code>{relation.kind}</code>
                    <span>{displayEnum(relation.lifecycle)}</span>
                  </div>
                  <code className="evidence-spine__relation-id">{relation.relation_id}</code>
                  <EvidenceDetails relation={relation} />
                </li>
              );
            })}
          </ol>
        )}
      </section>
    </section>
  );
}
