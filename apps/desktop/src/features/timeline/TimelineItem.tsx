import type { ChangeSubjectDto } from "../../generated/query/ChangeSubjectDto";
import type { TimelineItemDto } from "../../generated/query/TimelineItemDto";
import type { TimelineVersionLinkDto } from "../../generated/query/TimelineVersionLinkDto";

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

function subjectId(subject: ChangeSubjectDto): string {
  switch (subject.type) {
    case "resource":
      return subject.resource_id;
    case "relation":
      return subject.relation_id;
    case "binding":
      return subject.binding_id;
  }
}

function versionLinkId(link: TimelineVersionLinkDto): string {
  return link.type === "resource" ? link.resource_version_id : link.relation_version_id;
}

interface TimelineItemProps {
  readonly item: TimelineItemDto;
}

export function TimelineItem({ item }: TimelineItemProps) {
  const { change, version_links } = item;
  return (
    <article className="timeline-item">
      <div className="timeline-subject">
        <span className={`timeline-subject-badge timeline-subject-badge--${change.subject.type}`}>
          {subjectTypeLabels[change.subject.type]}
        </span>
        <code className="timeline-subject-id">{subjectId(change.subject)}</code>
        <code className="timeline-change-id">{change.change_id}</code>
      </div>
      {change.fields.map((field) => (
        <FieldDiff key={field.path} field={field} />
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
    </article>
  );
}
