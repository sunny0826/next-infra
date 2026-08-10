import { useEffect, useRef, useState, type SyntheticEvent } from "react";

import type { ChangeSubjectDto } from "../../generated/query/ChangeSubjectDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { TimelineItemDto } from "../../generated/query/TimelineItemDto";
import type { TimelineVersionLinkDto } from "../../generated/query/TimelineVersionLinkDto";
import type { DesktopAdapter } from "../../platform/desktop-adapter/desktop-adapter";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { EvidenceSpine } from "../evidence/EvidenceSpine";

import { FieldDiff } from "./FieldDiff";

/**
 * Subject types are a closed audit vocabulary, so their labels stay local
 * instead of depending on the shared enum label map.
 */
const subjectTypeLabels: Readonly<Record<ChangeSubjectDto["type"], string>> = {
  resource: "资源",
  relation: "关系",
  binding: "绑定",
};

const UNKNOWN_SUBJECT_LABEL = "未知";

function subjectTypeTone(type: ChangeSubjectDto["type"]): string {
  // The class suffix is derived only from the closed label map so a
  // malformed DTO type cannot inject an arbitrary class name.
  return type in subjectTypeLabels ? type : "unknown";
}

function subjectId(subject: ChangeSubjectDto): string | undefined {
  switch (subject.type) {
    case "resource":
      return subject.resource_id;
    case "relation":
      return subject.relation_id;
    case "binding":
      return subject.binding_id;
    default:
      // Unknown subject types (malformed DTO) have no id to display.
      return undefined;
  }
}

function versionLinkId(link: TimelineVersionLinkDto): string {
  return link.type === "resource" ? link.resource_version_id : link.relation_version_id;
}

interface TimelineItemProps {
  readonly item: TimelineItemDto;
}

/**
 * One spine per endpoint pair. Resource subjects resolve to their incident
 * relations grouped by `${source}\u0000${target}`; relation and binding
 * subjects cannot be resolved from the frontend alone (the adapter has no
 * relation lookup), so they render an honest explanation instead.
 */
interface EvidenceSpineData {
  readonly source: ResourceDto;
  readonly target: ResourceDto;
  readonly relations: readonly RelationDto[];
}

type EvidenceState =
  | { readonly status: "idle" }
  | { readonly status: "loading" }
  | { readonly status: "explanation"; readonly message: string }
  | { readonly status: "error" }
  | { readonly status: "ready"; readonly spines: readonly EvidenceSpineData[] };

const RESOURCE_EVIDENCE_ERROR = "无法读取此资源的证据。";
const RESOURCE_EVIDENCE_EMPTY = "此资源没有可展示的关系证据。";

function subjectExplanation(type: ChangeSubjectDto["type"]): string | undefined {
  switch (type) {
    case "relation":
      return "此变更涉及关系，缺少前端可解析的端点信息。";
    case "binding":
      return "此变更源于绑定，缺少可解析的关系端点。";
    default:
      // Unknown subject types (malformed DTO) have no known resolution path.
      return undefined;
  }
}

/**
 * Loads the subject resource and every relation incident to it, then groups
 * those relations by endpoint pair so each pair renders one EvidenceSpine.
 * Endpoint resources other than the subject are fetched so both sides of a
 * spine carry a full ResourceDto.
 */
async function resolveResourceEvidence(
  adapter: DesktopAdapter,
  subject: Extract<ChangeSubjectDto, { readonly type: "resource" }>,
): Promise<readonly EvidenceSpineData[]> {
  const detail = await adapter.getResource({
    resource_id: subject.resource_id,
    include: ["relations"],
  });
  const grouped = new Map<string, RelationDto[]>();
  for (const relation of detail.relations) {
    const key = `${relation.source_resource_id}\u0000${relation.target_resource_id}`;
    grouped.set(key, [...(grouped.get(key) ?? []), relation]);
  }
  const endpoints = new Map<string, ResourceDto>([
    [detail.resource.resource_id, detail.resource],
  ]);
  const missingEndpoints = new Set<string>();
  for (const relations of grouped.values()) {
    const first = relations[0];
    if (first.source_resource_id !== detail.resource.resource_id) {
      missingEndpoints.add(first.source_resource_id);
    }
    if (first.target_resource_id !== detail.resource.resource_id) {
      missingEndpoints.add(first.target_resource_id);
    }
  }
  await Promise.all(
    [...missingEndpoints].map(async (resourceId) => {
      const endpoint = await adapter.getResource({ resource_id: resourceId });
      endpoints.set(endpoint.resource.resource_id, endpoint.resource);
    }),
  );
  const spines: EvidenceSpineData[] = [];
  for (const relations of grouped.values()) {
    const source = endpoints.get(relations[0].source_resource_id);
    const target = endpoints.get(relations[0].target_resource_id);
    if (source !== undefined && target !== undefined) {
      spines.push({ source, target, relations });
    }
  }
  return spines;
}

export function TimelineItem({ item }: TimelineItemProps) {
  const adapter = useDesktopAdapter();
  const [evidence, setEvidence] = useState<EvidenceState>({ status: "idle" });
  // Guards against setState after unmount (page reload mid-fetch).
  const activeRef = useRef(true);
  useEffect(() => {
    return () => {
      activeRef.current = false;
    };
  }, []);

  function handleToggle(event: SyntheticEvent<HTMLDetailsElement>) {
    // Fetch only on the first open; later toggles reuse the resolved state.
    if (!event.currentTarget.open || evidence.status !== "idle") return;
    const subject = item.change.subject;
    if (subject.type !== "resource") {
      setEvidence({
        status: "explanation",
        message: subjectExplanation(subject.type) ?? RESOURCE_EVIDENCE_ERROR,
      });
      return;
    }
    setEvidence({ status: "loading" });
    resolveResourceEvidence(adapter, subject)
      .then((spines) => {
        if (activeRef.current) setEvidence({ status: "ready", spines });
      })
      .catch(() => {
        if (activeRef.current) setEvidence({ status: "error" });
      });
  }

  const { change, version_links } = item;
  const subjectType = change.subject.type;
  return (
    <article className="timeline-item">
      <div className="timeline-subject">
        <span
          className={`timeline-subject-badge timeline-subject-badge--${subjectTypeTone(subjectType)}`}
        >
          {subjectTypeLabels[subjectType] ?? UNKNOWN_SUBJECT_LABEL}
        </span>
        <code className="timeline-subject-id">{subjectId(change.subject) ?? "—"}</code>
        <code className="timeline-change-id">{change.change_id}</code>
      </div>
      {change.fields.map((field, index) => (
        <FieldDiff key={`${field.path}-${index}`} field={field} />
      ))}
      {version_links.length > 0 ? (
        <div className="timeline-links">
          {version_links.map((link) => (
            <code
              className="timeline-link-chip"
              key={`${link.type}-${versionLinkId(link)}`}
            >
              {versionLinkId(link)}
            </code>
          ))}
        </div>
      ) : null}
      <details className="timeline-evidence" onToggle={handleToggle}>
        <summary className="timeline-evidence-summary">证据链</summary>
        {evidence.status === "loading" ? (
          <p className="timeline-evidence-state">正在读取证据…</p>
        ) : evidence.status === "error" ? (
          <p className="timeline-evidence-state timeline-evidence-state--error">
            {RESOURCE_EVIDENCE_ERROR}
          </p>
        ) : evidence.status === "explanation" ? (
          <p className="timeline-evidence-state">{evidence.message}</p>
        ) : evidence.status === "ready" ? (
          evidence.spines.length === 0 ? (
            <p className="timeline-evidence-state">{RESOURCE_EVIDENCE_EMPTY}</p>
          ) : (
            <div className="timeline-evidence-spines">
              {evidence.spines.map((spine) => (
                <EvidenceSpine
                  key={`${spine.source.resource_id}-${spine.target.resource_id}`}
                  source={spine.source}
                  target={spine.target}
                  relations={spine.relations}
                />
              ))}
            </div>
          )
        ) : null}
      </details>
    </article>
  );
}
