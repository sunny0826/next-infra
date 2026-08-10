import { useId, useState } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import { EvidenceCard } from "./EvidenceCard";

import "./EvidenceSpine.css";

interface EvidenceSpineProps {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

/** Trust order: provider observations before manual bindings before inference. */
const TRUST_ORDER: Readonly<Record<RelationDto["evidence_type"], number>> = {
  provider: 0,
  configured: 1,
  inferred: 2,
};

/** Long evidence paths collapse to this many rows before the expand toggle. */
const VISIBLE_CUTOFF = 5;

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

/**
 * Source fact → evidence cards → target fact on a common vertical axis.
 * Every evidence row is listed; the last (current) step carries the cyan
 * active marker.
 */
export function EvidenceSpine({ source, target, relations }: EvidenceSpineProps) {
  const titleId = useId();
  const factsId = useId();
  const pathId = useId();
  const pathListId = useId();
  const [expanded, setExpanded] = useState(false);

  const ordered = [...relations].sort(
    (a, b) => TRUST_ORDER[a.evidence.type] - TRUST_ORDER[b.evidence.type],
  );
  const truncated = ordered.length > VISIBLE_CUTOFF;
  const visible = expanded ? ordered : ordered.slice(0, VISIBLE_CUTOFF);
  const lastVisibleId = visible.length > 0 ? visible[visible.length - 1].relation_id : undefined;

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

      <div className="evidence-spine__chain">
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
            <div className="evidence-spine__empty">
              <p>这些端点没有可用证据。</p>
              <p className="evidence-spine__empty-note">
                可能尚未被连接器观察，或关系已在同步中被移除。
              </p>
            </div>
          ) : (
            <>
              <ol id={pathListId} className="evidence-spine__path">
                {visible.map((relation) => (
                  <li
                    className={`evidence-spine__step${relation.relation_id === lastVisibleId ? " evidence-spine__step--active" : ""}`}
                    key={relation.relation_id}
                  >
                    <EvidenceCard direction="forward" relation={relation} />
                  </li>
                ))}
              </ol>
              {truncated ? (
                <button
                  type="button"
                  className="evidence-spine__toggle"
                  aria-controls={pathListId}
                  aria-expanded={expanded}
                  onClick={() => setExpanded((current) => !current)}
                >
                  {expanded ? "收起" : `展开全部 ${relations.length} 条证据`}
                </button>
              ) : null}
            </>
          )}
        </section>
      </div>
    </section>
  );
}
