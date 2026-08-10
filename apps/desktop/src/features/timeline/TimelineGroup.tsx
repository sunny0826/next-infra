import type { TimelineGroupDto } from "../../generated/query/TimelineGroupDto";
import type { TimelineOriginDto } from "../../generated/query/TimelineOriginDto";

import { formatRelativeTime } from "../overview/time";

import { TimelineItem } from "./TimelineItem";

const originLabels: Readonly<Record<TimelineOriginDto["type"], string>> = {
  sync_run: "同步运行",
  binding: "绑定",
  inference: "推断",
};

const UNKNOWN_ORIGIN_LABEL = "未知来源";

function originId(origin: TimelineOriginDto): string | undefined {
  switch (origin.type) {
    case "sync_run":
      return origin.sync_run_id;
    case "binding":
      return origin.binding_id;
    case "inference":
      return origin.rule_version;
    default:
      // Unknown origin types (malformed DTO) have no id to display.
      return undefined;
  }
}

function originTone(type: TimelineOriginDto["type"]): string {
  // The class suffix is derived only from the closed label map so a
  // malformed DTO type cannot inject an arbitrary class name.
  return type in originLabels ? type : "unknown";
}

interface TimelineGroupProps {
  readonly group: TimelineGroupDto;
}

export function TimelineGroup({ group }: TimelineGroupProps) {
  const type = group.origin.type;
  const label = originLabels[type] ?? UNKNOWN_ORIGIN_LABEL;
  const id = originId(group.origin);
  return (
    <section className="timeline-group" aria-label={id === undefined ? label : `${label} ${id}`}>
      <header className="timeline-group-header">
        <span
          aria-hidden="true"
          className={`timeline-origin-dot timeline-origin-dot--${originTone(type)}`}
        />
        <strong className="timeline-origin-label">{label}</strong>
        <code className="timeline-origin-id">{id ?? "—"}</code>
        <span className="timeline-group-count">{group.items.length} 项</span>
        <time
          className="timeline-group-time"
          dateTime={group.occurred_at}
          title={group.occurred_at}
        >
          {formatRelativeTime(group.occurred_at)}
        </time>
      </header>
      <div className="timeline-group-items">
        {group.items.map((item) => (
          <TimelineItem key={item.change.change_id} item={item} />
        ))}
      </div>
    </section>
  );
}
