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
    description: "Attention facts, connector observations, configured paths, and recent changes.",
  },
  {
    id: "inventory",
    label: "Inventory",
    icon: "inventory",
    description: "Bounded resources with stable filtering, selection, and pagination.",
  },
  {
    id: "topology",
    label: "Topology",
    icon: "topology",
    description: "A bounded focus-centered relation canvas with evidence labels and frontier limits.",
  },
  {
    id: "timeline",
    label: "Timeline",
    icon: "timeline",
    description: "Structured changes are unavailable until Goal 7 delivers the Timeline query.",
  },
  {
    id: "connectors",
    label: "Connectors",
    icon: "connectors",
    description: "Connection health, recent sync state, scheduling, and declared coverage.",
  },
  {
    id: "settings",
    label: "Settings",
    icon: "settings",
    description: "Local lifecycle, retention, data budget, and capability controls.",
  },
] as const;

export function getRoute(routeId: RouteId): ShellRoute {
  return SHELL_ROUTES.find((route) => route.id === routeId) ?? SHELL_ROUTES[0];
}
