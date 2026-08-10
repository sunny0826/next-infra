import { useEffect, useRef, useState } from "react";

import type { RelationDto } from "../../generated/query/RelationDto";
import { displayEnum } from "../../i18n";
import { formatRelativeTime, humanizeKind, middleTruncate } from "../../lib/format";
import { Icon } from "../../ui/Icon";

import "./EvidenceCard.css";

const evidenceLabels = {
  provider: "提供方",
  configured: "已配置",
  inferred: "推断",
} as const;

const evidenceExplanations: Readonly<Record<RelationDto["evidence_type"], string>> = {
  provider: "Provider 直接观察到的关系",
  configured: "手动配置的绑定，不伪造同步来源",
  inferred: "可复现的规则推断，置信度按推理给出",
};

interface EvidenceCardProps {
  readonly relation: RelationDto;
  /** Renders a direction arrow toward the target fact; omitted outside the spine. */
  readonly direction?: "forward" | "backward";
  /** Starts with the details block expanded (default: collapsed). */
  readonly defaultOpen?: boolean;
}

interface CopyableIdProps {
  /** Full raw identifier — copied verbatim and exposed as the hover title. */
  readonly value: string;
  /**
   * Pre-shortened display text (middleTruncate applied by the caller) so long
   * IDs keep the compact ellipsis form; defaults to the full value for rows
   * that are intentionally rendered verbatim.
   */
  readonly display?: string;
}

/**
 * Renders an identifier with a ghost copy button beside it, shown only for
 * long values (> 24 chars). The displayed text is never hidden or replaced —
 * the button just sits after the code so searchable fixture IDs stay intact.
 * Copying writes the full value and swaps the button to "已复制" for ~1.2s.
 */
function CopyableId({ value, display = value }: CopyableIdProps) {
  const [copied, setCopied] = useState(false);
  const timeoutRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    return () => window.clearTimeout(timeoutRef.current);
  }, []);

  async function handleCopy() {
    try {
      await navigator.clipboard?.writeText(value);
    } catch {
      return;
    }
    window.clearTimeout(timeoutRef.current);
    setCopied(true);
    timeoutRef.current = window.setTimeout(() => setCopied(false), 1200);
  }

  return (
    <span className="evidence-card__copyable">
      <code title={value}>{display}</code>
      {value.length > 24 ? (
        <button
          type="button"
          className="evidence-card__copy"
          aria-label={`复制 ${value}`}
          onClick={() => void handleCopy()}
        >
          {copied ? (
            <span className="evidence-card__copy-done">已复制</span>
          ) : (
            <Icon name="copy" />
          )}
        </button>
      ) : null}
    </span>
  );
}

function evidenceSummary(relation: RelationDto): string {
  const evidence = relation.evidence;
  if (evidence.type === "provider") {
    return `提供方证据 · ${evidence.connector_type} 连接 · ${formatRelativeTime(relation.last_seen_at)}观察`;
  }
  if (evidence.type === "configured") {
    return `已配置证据 · 手动绑定 · ${formatRelativeTime(evidence.created_at)}创建`;
  }
  return `推断证据 · ${evidence.confidence_basis_points / 100}% 置信度`;
}

function Confidence({ basisPoints }: { readonly basisPoints: number }) {
  const percentage = basisPoints / 100;
  return (
    <span className="evidence-card__confidence">
      <span aria-hidden="true" className="evidence-card__confidence-track">
        <span
          className="evidence-card__confidence-fill"
          style={{ width: `${Math.min(100, percentage)}%` }}
        />
      </span>
      <code>{basisPoints} bp</code>
    </span>
  );
}

/**
 * Renders one evidence entry for a relation. Shared by the Evidence Spine and
 * the Inspector so provenance rendering is never duplicated.
 */
export function EvidenceCard({ relation, direction, defaultOpen = false }: EvidenceCardProps) {
  const evidence = relation.evidence;
  const type = evidence.type;

  return (
    <article
      aria-label={`${evidenceLabels[type]} evidence`}
      className={`evidence-card evidence-card--${type}`}
    >
      <header className="evidence-card__head">
        <span className={`evidence-card__badge evidence-card__badge--${type}`}>
          <span aria-hidden="true" className="evidence-card__badge-dot" />
          <span className="evidence-card__badge-label">{evidenceLabels[type]}</span>
          <span className="evidence-card__badge-note">{evidenceExplanations[type]}</span>
        </span>
        {direction !== undefined ? (
          <span aria-hidden="true" className="evidence-card__arrow">
            {direction === "forward" ? "→" : "←"}
          </span>
        ) : null}
        <span className="evidence-card__kind">{humanizeKind(relation.kind)}</span>
        <span className="evidence-card__lifecycle">{displayEnum(relation.lifecycle)}</span>
      </header>

      <details className="evidence-card__details" open={defaultOpen || undefined}>
        <summary className="evidence-card__summary">{evidenceSummary(relation)}</summary>
        <dl className="evidence-card__facts">
          <dt>关系类型</dt>
          <dd><code>{relation.kind}</code></dd>
          <dt>关系</dt>
          <dd><CopyableId value={relation.relation_id} /></dd>
          {type === "provider" ? (
            <>
              <dt>连接器</dt>
              <dd><code>{evidence.connector_type}</code></dd>
              <dt>连接</dt>
              <dd><CopyableId value={evidence.connection_id} display={middleTruncate(evidence.connection_id)} /></dd>
              <dt>同步运行</dt>
              <dd><CopyableId value={evidence.sync_run_id} display={middleTruncate(evidence.sync_run_id)} /></dd>
              <dt>字段路径</dt>
              <dd><CopyableId value={evidence.field_path} display={middleTruncate(evidence.field_path)} /></dd>
            </>
          ) : null}
          {type === "configured" ? (
            <>
              <dt>绑定</dt>
              <dd><CopyableId value={evidence.binding_id} display={middleTruncate(evidence.binding_id)} /></dd>
              <dt>创建时间</dt>
              <dd><time dateTime={evidence.created_at}>{evidence.created_at}</time></dd>
            </>
          ) : null}
          {type === "inferred" ? (
            <>
              <dt>规则版本</dt>
              <dd><CopyableId value={evidence.rule_version} display={middleTruncate(evidence.rule_version)} /></dd>
              <dt>输入资源版本</dt>
              <dd>
                {evidence.input_resource_version_ids.length === 0 ? (
                  <span>无</span>
                ) : (
                  <ul>
                    {evidence.input_resource_version_ids.map((versionId) => (
                      <li key={versionId}><CopyableId value={versionId} display={middleTruncate(versionId)} /></li>
                    ))}
                  </ul>
                )}
              </dd>
              <dt>输入关系版本</dt>
              <dd>
                {evidence.input_relation_version_ids.length === 0 ? (
                  <span>无</span>
                ) : (
                  <ul>
                    {evidence.input_relation_version_ids.map((versionId) => (
                      <li key={versionId}><CopyableId value={versionId} display={middleTruncate(versionId)} /></li>
                    ))}
                  </ul>
                )}
              </dd>
              <dt>置信度</dt>
              <dd><Confidence basisPoints={evidence.confidence_basis_points} /></dd>
            </>
          ) : null}
          <dt>最近观测</dt>
          <dd><time dateTime={relation.last_seen_at}>{relation.last_seen_at}</time></dd>
        </dl>
      </details>
    </article>
  );
}
