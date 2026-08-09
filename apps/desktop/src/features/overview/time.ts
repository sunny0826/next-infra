const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * Formats an ISO timestamp as a compact zh-CN relative time.
 *
 * Invalid input is returned verbatim so a malformed snapshot value never
 * renders as a fabricated timestamp. `now` is injectable for deterministic
 * tests; the trailing compact date uses UTC so the output is timezone-stable.
 */
export function formatRelativeTime(iso: string, now: Date = new Date()): string {
  const time = new Date(iso).getTime();
  if (Number.isNaN(time)) return iso;

  const diffMs = now.getTime() - time;
  if (diffMs < MINUTE_MS) return "刚刚";

  const minutes = Math.floor(diffMs / MINUTE_MS);
  if (minutes < 60) return `${minutes} 分钟前`;

  const hours = Math.floor(diffMs / HOUR_MS);
  if (hours < 24) return `${hours} 小时前`;

  const days = Math.floor(diffMs / DAY_MS);
  if (days < 7) return `${days} 天前`;

  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks} 周前`;

  const date = new Date(time);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
