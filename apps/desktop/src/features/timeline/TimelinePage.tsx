import { useEffect, useState } from "react";

import type { TimelineGroupDto } from "../../generated/query/TimelineGroupDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./timeline.css";

function originLabel(group: TimelineGroupDto): string {
  switch (group.origin.type) {
    case "sync_run":
      return `同步 ${group.origin.sync_run_id}`;
    case "binding":
      return `绑定 ${group.origin.binding_id}`;
    case "inference":
      return `推断 ${group.origin.rule_version}`;
  }
}

function compactValue(value: unknown): string {
  const text = JSON.stringify(value);
  return text.length > 160 ? `${text.slice(0, 160)}...` : text;
}

export function TimelinePage() {
  const adapter = useDesktopAdapter();
  const [groups, setGroups] = useState<readonly TimelineGroupDto[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = (nextCursor?: string) => {
    setLoading(true);
    setError(null);
    adapter
      .getTimeline(nextCursor === undefined ? {} : { cursor: nextCursor, limit: 50 })
      .then((page) => {
        setGroups((current) => nextCursor === undefined ? page.groups : [...current, ...page.groups]);
        setCursor(page.page_info.next_cursor);
      })
      .catch(() => setError("无法读取本地变更时间线。"))
      .finally(() => setLoading(false));
  };

  useEffect(() => { load(); }, [adapter]);

  return (
    <section className="timeline-page" aria-labelledby="timeline-title">
      <header className="timeline-header">
        <div><p className="timeline-eyebrow">已提交的变更历史</p><h1 id="timeline-title">时间线</h1></div>
      </header>
      {error !== null ? <p className="timeline-error" role="alert">{error}</p> : null}
      <div className="timeline-scroll" aria-busy={loading}>
        {groups.map((group) => (
          <section className="timeline-group" key={group.group_id}>
            <header><strong>{originLabel(group)}</strong><time dateTime={group.occurred_at}>{group.occurred_at}</time></header>
            {group.items.map((item) => (
              <article className="timeline-item" key={item.change.change_id}>
                <div className="timeline-subject"><code>{item.change.change_id}</code><span>{item.change.subject.type}</span></div>
                {item.change.fields.map((field) => (
                  <details className="timeline-diff" key={field.path}>
                    <summary>{field.path}</summary>
                    <div><code>变更前</code><pre>{compactValue(field.before)}</pre></div>
                    <div><code>变更后</code><pre>{compactValue(field.after)}</pre></div>
                  </details>
                ))}
                {item.version_links.length > 0 ? <div className="timeline-links">
                  {item.version_links.map((link) => <code key={`${link.type}-${link.type === "resource" ? link.resource_version_id : link.relation_version_id}`}>{link.type === "resource" ? link.resource_version_id : link.relation_version_id}</code>)}
                </div> : null}
              </article>
            ))}
          </section>
        ))}
        {!loading && groups.length === 0 && error === null ? <p className="timeline-empty">没有已持久化的变更。</p> : null}
      </div>
      {cursor !== null ? <button className="timeline-load-more" disabled={loading} onClick={() => load(cursor)} type="button">加载更多</button> : null}
    </section>
  );
}
