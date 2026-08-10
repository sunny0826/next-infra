import { useEffect, useRef, useState } from "react";

import type { TimelineGroupDto } from "../../generated/query/TimelineGroupDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import { TimelineGroup } from "./TimelineGroup";

import "./timeline.css";

const PAGE_SIZE = 50;
const INITIAL_ERROR_MESSAGE = "无法读取本地变更时间线。";
const LOAD_MORE_ERROR_MESSAGE = "无法加载更多变更。";

/**
 * Appends one fetched page to the loaded groups. The backend can cut a
 * logical group at the page boundary; when the first incoming group is the
 * continuation of the last loaded group (identical group_id), its items are
 * merged into that group instead of rendering a duplicate section.
 */
function appendPageGroups(
  current: readonly TimelineGroupDto[],
  incoming: readonly TimelineGroupDto[],
): TimelineGroupDto[] {
  const merged = [...current];
  const remaining = [...incoming];
  const last = merged[merged.length - 1];
  const first = remaining[0];
  if (last !== undefined && first !== undefined && first.group_id === last.group_id) {
    merged[merged.length - 1] = { ...last, items: [...last.items, ...first.items] };
    remaining.shift();
  }
  return [...merged, ...remaining];
}

interface TimelinePageProps {
  readonly queryVersion?: number;
}

export function TimelinePage({ queryVersion = 0 }: TimelinePageProps) {
  const adapter = useDesktopAdapter();
  const [groups, setGroups] = useState<readonly TimelineGroupDto[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [initialError, setInitialError] = useState<string | null>(null);
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null);
  // Monotonic request sequence: only the newest load may commit state, so a
  // late-resolving stale response never clobbers a fresher one.
  const sequenceRef = useRef(0);

  const load = (nextCursor?: string) => {
    const sequence = ++sequenceRef.current;
    const isInitial = nextCursor === undefined;
    setLoading(true);
    // A fresh load supersedes both error states; a load-more only its own.
    setInitialError(null);
    setLoadMoreError(null);
    adapter
      .getTimeline(isInitial ? {} : { cursor: nextCursor, limit: PAGE_SIZE })
      .then((page) => {
        if (sequence !== sequenceRef.current) return;
        setGroups((current) =>
          isInitial ? [...page.groups] : appendPageGroups(current, page.groups),
        );
        setCursor(page.page_info.next_cursor);
      })
      .catch(() => {
        if (sequence !== sequenceRef.current) return;
        if (isInitial) setInitialError(INITIAL_ERROR_MESSAGE);
        else setLoadMoreError(LOAD_MORE_ERROR_MESSAGE);
      })
      .finally(() => {
        if (sequence !== sequenceRef.current) return;
        setLoading(false);
      });
  };

  useEffect(() => {
    load();
  }, [adapter, queryVersion]);

  const itemCount = groups.reduce((total, group) => total + group.items.length, 0);
  const showSkeleton = loading && groups.length === 0 && initialError === null;
  // Empty state is only valid when the backend also says there is nothing
  // left; an empty page carrying a cursor (backend anomaly) keeps the
  // load-more affordance instead of contradicting it with an empty message.
  const showEmpty =
    !loading && groups.length === 0 && cursor === null && initialError === null;

  return (
    <section className="timeline-page" aria-labelledby="timeline-title">
      <header className="timeline-header">
        <div>
          <p className="timeline-eyebrow">已提交的变更历史</p>
          <h1 id="timeline-title">时间线</h1>
        </div>
      </header>
      {initialError !== null ? (
        <div className="timeline-banner" role="alert">
          <span>{initialError}</span>
          <button className="timeline-retry" type="button" onClick={() => load()}>
            重试
          </button>
        </div>
      ) : null}
      <div className="timeline-scroll" aria-busy={loading}>
        {showSkeleton ? (
          <div className="timeline-skeleton" aria-hidden="true">
            <div className="timeline-skeleton-row" />
            <div className="timeline-skeleton-row" />
            <div className="timeline-skeleton-row" />
          </div>
        ) : null}
        {groups.map((group) => (
          <TimelineGroup key={group.group_id} group={group} />
        ))}
        {showEmpty ? (
          <div className="timeline-empty">
            <p className="timeline-empty-primary">没有已持久化的变更。</p>
            <p className="timeline-empty-hint">完成一次同步或建立绑定后，审计记录会出现在这里。</p>
          </div>
        ) : null}
      </div>
      {cursor !== null || itemCount > 0 ? (
        <div className="timeline-load-more-row">
          {cursor !== null ? (
            <button
              className="timeline-load-more"
              type="button"
              disabled={loading}
              onClick={() => load(cursor)}
            >
              加载更多
            </button>
          ) : null}
          {loadMoreError !== null ? (
            <span className="timeline-load-more-error">{loadMoreError}</span>
          ) : null}
          {itemCount > 0 ? (
            <span className="timeline-loaded-count">已加载 {itemCount} 项变更</span>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
