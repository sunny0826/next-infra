import type { IconName } from "../ui/Icon";

export type RouteId =
  | "overview"
  | "inventory"
  | "topology"
  | "timeline"
  | "connectors"
  | "settings";

export interface ShellRoute {
  id: RouteId;
  label: string;
  icon: IconName;
  description: string;
}

export const SHELL_ROUTES: readonly ShellRoute[] = [
  {
    id: "overview",
    label: "概览",
    icon: "overview",
    description: "需要关注的事实、连接器观测、已配置路径和近期变更。",
  },
  {
    id: "inventory",
    label: "资源清单",
    icon: "inventory",
    description: "可筛选、选择和分页浏览的受限资源。",
  },
  {
    id: "topology",
    label: "拓扑",
    icon: "topology",
    description: "以焦点资源为中心、带证据标签和边界限制的关系画布。",
  },
  {
    id: "timeline",
    label: "时间线",
    icon: "timeline",
    description: "按持久化来源分组的已提交结构化变更。",
  },
  {
    id: "connectors",
    label: "连接器",
    icon: "connectors",
    description: "连接健康度、最近同步状态、计划和声明覆盖范围。",
  },
  {
    id: "settings",
    label: "设置",
    icon: "settings",
    description: "本地生命周期、保留策略、数据预算和能力控制。",
  },
] as const;

export function getRoute(routeId: RouteId): ShellRoute {
  return SHELL_ROUTES.find((route) => route.id === routeId) ?? SHELL_ROUTES[0];
}
