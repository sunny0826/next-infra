export interface ManualRelationKindOption {
  readonly id:
    | "infra.deployed_via"
    | "infra.accessed_via"
    | "network.routes_to"
    | "automation.deploys_to"
    | "data.writes_to"
    | "infra.depends_on";
  readonly label: string;
  readonly sourceHint: string;
  readonly targetHint: string;
}

export const MANUAL_RELATION_KIND_OPTIONS = [
  {
    id: "infra.deployed_via",
    label: "通过目标控制面部署",
    sourceHint: "被部署的资源",
    targetHint: "控制面或部署应用",
  },
  {
    id: "infra.accessed_via",
    label: "通过目标入口访问",
    sourceHint: "需要访问的资源",
    targetHint: "入口或访问主机",
  },
  {
    id: "network.routes_to",
    label: "路由到目标",
    sourceHint: "路由记录或入口",
    targetHint: "接收流量的资源",
  },
  {
    id: "automation.deploys_to",
    label: "自动化工作流部署到目标",
    sourceHint: "自动化工作流",
    targetHint: "部署目标",
  },
  {
    id: "data.writes_to",
    label: "声明写入目标数据服务",
    sourceHint: "写入方或工作流",
    targetHint: "数据服务",
  },
  {
    id: "infra.depends_on",
    label: "依赖目标",
    sourceHint: "依赖方",
    targetHint: "被依赖资源",
  },
] as const satisfies readonly ManualRelationKindOption[];

export type ManualRelationKind =
  (typeof MANUAL_RELATION_KIND_OPTIONS)[number]["id"];

export function getManualRelationKindOption(
  kind: string | null | undefined,
): ManualRelationKindOption | null {
  return (
    MANUAL_RELATION_KIND_OPTIONS.find((option) => option.id === kind) ?? null
  );
}
