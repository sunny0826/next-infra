/**
 * Formatting helpers shared by evidence views.
 *
 * All outputs are locale-stable and only transform values that already exist
 * in the projection DTOs — they never embed hostnames, repository names,
 * credentials, or fixture content.
 */

const RELATIVE_UNITS: readonly [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
];

const zhRelativeTime = new Intl.RelativeTimeFormat("zh-CN", { numeric: "auto" });

/**
 * Renders an ISO timestamp as a compact zh-CN relative time ("刚刚",
 * "5 分钟前", "2 小时前", "3 天前"). Falls back to the raw value when the
 * timestamp cannot be parsed.
 */
export function formatRelativeTime(iso: string, now: Date = new Date()): string {
  const parsed = Date.parse(iso);
  if (Number.isNaN(parsed)) return iso;
  const delta = parsed - now.getTime();
  const distance = Math.abs(delta);
  if (distance < 60 * 1000) return "刚刚";
  for (const [unit, span] of RELATIVE_UNITS) {
    if (distance >= span) {
      return zhRelativeTime.format(Math.round(delta / span), unit);
    }
  }
  return "刚刚";
}

/**
 * Collapses long identifiers by keeping both ends and replacing the middle
 * with an ellipsis. Values at or below `maxLength` are returned unchanged so
 * stable fixture IDs stay searchable verbatim.
 */
export function middleTruncate(value: string, maxLength = 32): string {
  if (value.length <= maxLength) return value;
  const head = Math.ceil((maxLength - 1) / 2);
  const tail = maxLength - 1 - head;
  return `${value.slice(0, head)}…${value.slice(value.length - tail)}`;
}

const KIND_LABELS: Readonly<Record<string, string>> = {
  accessed_via: "经入口访问",
  contains: "包含",
  depends_on: "依赖",
  deployed_via: "部署",
  deploys_to: "部署到",
  executes: "执行",
  routes_to: "路由到",
  writes_to: "写入",
};

/**
 * Maps a relation kind to a short human label, falling back to the raw kind
 * (the raw kind always remains available in the evidence details).
 */
export function humanizeKind(kind: string): string {
  const segment = kind.split(".").pop();
  return segment !== undefined ? (KIND_LABELS[segment] ?? kind) : kind;
}
