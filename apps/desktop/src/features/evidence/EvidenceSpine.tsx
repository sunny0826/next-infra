import { useId } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";

import "./EvidenceSpine.css";

interface EvidenceSpineProps {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

const evidenceLabels = {
  provider: "Provider",
  configured: "Configured",
  inferred: "Inferred",
} as const;

function CurrentFact({
  label,
  resource,
}: {
  readonly label: string;
  readonly resource: ResourceDto;
}) {
  return (
    <article className="evidence-spine__fact" aria-label={`${label} current fact`}>
      <span className="evidence-spine__eyebrow">{label}</span>
      <strong>{resource.display_name}</strong>
      <code>{resource.resource_id}</code>
      <div className="evidence-spine__status-line">
        <span><small>Health</small>{resource.health}</span>
        <span><small>Freshness</small>{resource.freshness}</span>
        <span><small>Lifecycle</small>{resource.lifecycle}</span>
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
        <dt>Provider</dt>
        <dd><code>{evidence.connector_type}</code></dd>
        <dt>Connection</dt>
        <dd><code>{evidence.connection_id}</code></dd>
        <dt>SyncRun</dt>
        <dd><code>{evidence.sync_run_id}</code></dd>
        <dt>Field path</dt>
        <dd><code>{evidence.field_path}</code></dd>
        <dt>Last seen</dt>
        <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
      </dl>
    );
  }

  if (evidence.type === "configured") {
    return (
      <dl className="evidence-spine__details">
        <dt>Binding</dt>
        <dd><code>{evidence.binding_id}</code></dd>
        <dt>Created</dt>
        <dd><time dateTime={evidence.created_at}>{evidence.created_at}</time></dd>
        <dt>Last seen</dt>
        <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
      </dl>
    );
  }

  return (
    <dl className="evidence-spine__details">
      <dt>Rule version</dt>
      <dd><code>{evidence.rule_version}</code></dd>
      <dt>Input versions</dt>
      <dd>
        <ul className="evidence-spine__inputs" aria-label="Input resource versions">
          {evidence.input_resource_version_ids.map((versionId) => (
            <li key={versionId}><code>{versionId}</code></li>
          ))}
        </ul>
      </dd>
      <dt>Relation inputs</dt>
      <dd>
        {evidence.input_relation_version_ids.length === 0 ? (
          <span>None</span>
        ) : (
          <ul className="evidence-spine__inputs" aria-label="Input relation versions">
            {evidence.input_relation_version_ids.map((versionId) => (
              <li key={versionId}><code>{versionId}</code></li>
            ))}
          </ul>
        )}
      </dd>
      <dt>Confidence</dt>
      <dd><Confidence basisPoints={evidence.confidence_basis_points} /></dd>
      <dt>Last seen</dt>
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
          <span className="evidence-spine__eyebrow">Inspector</span>
          <h2 id={titleId}>Evidence Spine</h2>
        </div>
        <span className="evidence-spine__count">
          {relations.length} {relations.length === 1 ? "source" : "sources"}
        </span>
      </header>

      <section className="evidence-spine__section" aria-labelledby={factsId}>
        <h3 id={factsId}>Current Facts</h3>
        <div className="evidence-spine__facts">
          <CurrentFact label="Source" resource={source} />
          <CurrentFact label="Target" resource={target} />
        </div>
      </section>

      <section className="evidence-spine__section" aria-labelledby={pathId}>
        <h3 id={pathId}>Evidence Path</h3>
        {relations.length === 0 ? (
          <p className="evidence-spine__empty">No evidence is available for these endpoints.</p>
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
                    <span>{relation.lifecycle}</span>
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
