export const DEFAULT_LOCALE = "zh-CN";

const enumLabels: Readonly<Record<string, string>> = {
  active: "活动",
  authoritative_full: "权威完整",
  auth_failed: "认证失败",
  available: "可用",
  bounded: "受限",
  cancelled: "已取消",
  clear: "未锁定",
  configured: "已配置",
  degraded: "降级",
  disabled: "已禁用",
  expired: "已过期",
  failed: "失败",
  fresh: "新鲜",
  full: "全量",
  healthy: "健康",
  incremental: "增量",
  inferred: "推断",
  interrupted: "已中断",
  latched: "已锁定",
  none: "无",
  orphaned: "孤立",
  partial: "部分覆盖",
  provider: "提供方",
  rate_limited: "限流中",
  recovery: "恢复",
  running: "运行中",
  schedule: "计划",
  stale: "已过时",
  startup: "启动",
  supported: "支持",
  succeeded: "成功",
  targeted: "定向",
  truncated: "已截断",
  unavailable: "不可用",
  unhealthy: "不健康",
  unknown: "未知",
  unreachable: "不可达",
  user: "手动",
};

export function displayEnum(value: string): string {
  return enumLabels[value] ?? value;
}

export function displayRuntimeReason(value: string): string {
  if (value.includes("Explicit Quit is latched")) {
    return "已锁定明确退出。请交互式重新打开 Next Infra，或等待下一次启用的登录启动以解除抑制。";
  }
  if (value.includes("Trusted MCP integration is installed")) {
    return "已安装并授权受信任的 MCP 集成。";
  }
  if (value.includes("Trusted MCP integration is not installed")) {
    return "尚未安装、启用或验证受信任的 MCP 集成。";
  }
  return value;
}

export function initializeLocale(): void {
  document.documentElement.lang = DEFAULT_LOCALE;
}
