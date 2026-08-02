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
    label: "Overview",
    icon: "overview",
    description: "Attention and snapshot context will be registered by the Overview page owner.",
  },
  {
    id: "inventory",
    label: "Inventory",
    icon: "inventory",
    description: "The bounded resource table will be registered by the Inventory page owner.",
  },
  {
    id: "topology",
    label: "Topology",
    icon: "topology",
    description: "The focus-centered relation canvas will be registered by the Topology page owner.",
  },
  {
    id: "timeline",
    label: "Timeline",
    icon: "timeline",
    description: "Structured change groups will be registered by the Timeline page owner.",
  },
  {
    id: "connectors",
    label: "Connectors",
    icon: "connectors",
    description: "Connection and coverage surfaces will be registered by the Connectors page owner.",
  },
  {
    id: "settings",
    label: "Settings",
    icon: "settings",
    description: "Local-only controls will be registered by the Settings page owner.",
  },
] as const;

export function getRoute(routeId: RouteId): ShellRoute {
  return SHELL_ROUTES.find((route) => route.id === routeId) ?? SHELL_ROUTES[0];
}
