import { useEffect, useId, useMemo, useState, type FormEvent } from "react";

import type { ConnectionDto } from "../../../generated/query/ConnectionDto";
import type { RelationDto } from "../../../generated/query/RelationDto";
import type { ResourceDto } from "../../../generated/query/ResourceDto";
import {
  desktopErrorCode,
  type SearchResourcesInput,
} from "../../../platform/desktop-adapter/desktop-adapter";
import { useDesktopAdapter } from "../../../platform/desktop-adapter/DesktopAdapterContext";

import {
  getManualRelationKindOption,
  MANUAL_RELATION_KIND_OPTIONS,
  type ManualRelationKind,
} from "./relation-vocabulary";

import "./relation-builder.css";

export interface RelationBuilderProps {
  source?: ResourceDto | null;
  relation?: RelationDto | null;
  onSaved: (result: RelationMutationResult) => void;
  onCancel: () => void;
}

export interface RelationMutationResult {
  readonly action: "created" | "updated" | "disabled" | "existing";
  readonly sourceResourceId: string;
  readonly targetResourceId: string;
  readonly kind: string;
}

type ResourcePickerState =
  | { readonly type: "idle" }
  | { readonly type: "loading" }
  | { readonly type: "error" }
  | { readonly type: "ready"; readonly items: readonly ResourceDto[] };

interface ResourcePickerProps {
  readonly label: string;
  readonly selectedId: string;
  readonly selectedResource: ResourceDto | null;
  readonly onSelect: (resource: ResourceDto) => void;
  readonly disabled: boolean;
}

const SEARCH_LIMIT = 20;

function resourceLabel(resource: ResourceDto | null, resourceId: string): string {
  return (resource?.display_name ?? resourceId) || "选择资源";
}

function uniqueConnections(connections: readonly ConnectionDto[]): readonly string[] {
  return [...new Set(connections.map((connection) => connection.connector_type))].sort();
}

function ResourcePicker({
  label,
  selectedId,
  selectedResource,
  onSelect,
  disabled,
}: ResourcePickerProps) {
  const adapter = useDesktopAdapter();
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [connectorFilter, setConnectorFilter] = useState("");
  const [kindFilter, setKindFilter] = useState("");
  const [connections, setConnections] = useState<readonly ConnectionDto[]>([]);
  const [state, setState] = useState<ResourcePickerState>({ type: "idle" });

  useEffect(() => {
    if (!open) return;
    let active = true;
    adapter.listConnections().then((page) => {
      if (active) setConnections(page.items);
    }).catch(() => {
      if (active) setConnections([]);
    });
    return () => {
      active = false;
    };
  }, [adapter, open]);

  useEffect(() => {
    if (!open) return;
    let active = true;
    const input: SearchResourcesInput = {
      query: query.trim() || undefined,
      connector_types: connectorFilter ? [connectorFilter] : undefined,
      kinds: kindFilter.trim() ? [kindFilter.trim()] : undefined,
      limit: SEARCH_LIMIT,
    };
    setState({ type: "loading" });
    adapter.searchResources(input).then((page) => {
      if (active) setState({ type: "ready", items: page.items });
    }).catch(() => {
      if (active) setState({ type: "error" });
    });
    return () => {
      active = false;
    };
  }, [adapter, connectorFilter, kindFilter, open, query]);

  const connectorOptions = useMemo(
    () => uniqueConnections(connections),
    [connections],
  );
  const kindOptions = useMemo(() => {
    if (state.type !== "ready") return [];
    return [...new Set(state.items.map((resource) => resource.kind))].sort();
  }, [state]);
  const listboxId = `${panelId}-options`;

  return (
    <div className="relation-resource-picker">
      <button
        aria-controls={panelId}
        aria-expanded={open}
        aria-label={`${label}：${resourceLabel(selectedResource, selectedId)}`}
        className="relation-resource-picker-trigger"
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <span className="relation-resource-picker-label">{label}</span>
        <span className="relation-resource-picker-value">
          {resourceLabel(selectedResource, selectedId)}
          {selectedId && selectedResource === null ? (
            <code>{selectedId}</code>
          ) : null}
        </span>
        <span aria-hidden="true">{open ? "▴" : "▾"}</span>
      </button>

      {open ? (
        <div className="relation-picker-panel" id={panelId}>
          <div className="relation-picker-inputs" role="search">
            <label>
              <span>资源查询</span>
              <input
                aria-label={`${label}查询`}
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="名称、类型或本地标识"
                type="search"
                value={query}
              />
            </label>
            <label>
              <span>类型筛选</span>
              <input
                aria-label={`${label}类型筛选`}
                onChange={(event) => setKindFilter(event.currentTarget.value)}
                placeholder="资源类型"
                type="search"
                value={kindFilter}
              />
            </label>
          </div>

          <div className="relation-picker-filter" role="group" aria-label={`${label}连接器筛选`}>
            <span>连接器</span>
            <div className="relation-picker-filter-options">
              <button
                aria-pressed={connectorFilter === ""}
                className={connectorFilter === "" ? "is-active" : ""}
                onClick={() => setConnectorFilter("")}
                type="button"
              >
                全部
              </button>
              {connectorOptions.map((connectorType) => (
                <button
                  aria-pressed={connectorFilter === connectorType}
                  className={connectorFilter === connectorType ? "is-active" : ""}
                  key={connectorType}
                  onClick={() => setConnectorFilter(connectorType)}
                  type="button"
                >
                  {connectorType}
                </button>
              ))}
            </div>
          </div>

          <div className="relation-picker-filter" role="group" aria-label={`${label}类型快捷筛选`}>
            <span>类型快捷筛选</span>
            <div className="relation-picker-filter-options">
              <button
                aria-pressed={kindFilter === ""}
                className={kindFilter === "" ? "is-active" : ""}
                onClick={() => setKindFilter("")}
                type="button"
              >
                全部
              </button>
              {kindOptions.map((kind) => (
                <button
                  aria-pressed={kindFilter === kind}
                  className={kindFilter === kind ? "is-active" : ""}
                  key={kind}
                  onClick={() => setKindFilter(kind)}
                  type="button"
                >
                  {kind}
                </button>
              ))}
            </div>
          </div>

          {state.type === "loading" ? (
            <p aria-busy="true" className="relation-picker-state">正在查询受限资源…</p>
          ) : null}
          {state.type === "error" ? (
            <p className="relation-picker-state relation-picker-state--error" role="alert">
              无法读取本地资源清单。
            </p>
          ) : null}
          {state.type === "ready" && state.items.length === 0 ? (
            <p className="relation-picker-state">没有匹配的本地资源。</p>
          ) : null}
          {state.type === "ready" && state.items.length > 0 ? (
            <ul aria-label={`${label}资源选项`} className="relation-picker-options" id={listboxId} role="listbox">
              {state.items.map((resource) => (
                <li key={resource.resource_id}>
                  <button
                    aria-selected={resource.resource_id === selectedId}
                    className={resource.resource_id === selectedId ? "is-selected" : ""}
                    onClick={() => {
                      onSelect(resource);
                      setOpen(false);
                    }}
                    role="option"
                    type="button"
                  >
                    <strong>{resource.display_name}</strong>
                    <code>{resource.kind} · {resource.resource_id}</code>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function endpointId(
  relation: RelationDto | null | undefined,
  endpoint: "source" | "target",
): string {
  if (relation === null || relation === undefined) return "";
  return endpoint === "source"
    ? relation.source_resource_id
    : relation.target_resource_id;
}

export function RelationBuilder({
  source,
  relation,
  onSaved,
  onCancel,
}: RelationBuilderProps) {
  const adapter = useDesktopAdapter();
  const [sourceId, setSourceId] = useState(
    relation?.source_resource_id ?? source?.resource_id ?? "",
  );
  const [targetId, setTargetId] = useState(endpointId(relation, "target"));
  const [sourceResource, setSourceResource] = useState<ResourceDto | null>(
    relation === null || relation === undefined || relation.source_resource_id === source?.resource_id
      ? source ?? null
      : null,
  );
  const [targetResource, setTargetResource] = useState<ResourceDto | null>(null);
  const [kind, setKind] = useState<string>(
    relation?.kind ?? MANUAL_RELATION_KIND_OPTIONS[0].id,
  );
  const [pending, setPending] = useState<"idle" | "saving" | "disabling">("idle");
  const [error, setError] = useState<string | null>(null);

  const configuredRelation = relation?.evidence.type === "configured" ? relation : null;
  const configuredBindingId =
    relation?.evidence.type === "configured" ? relation.evidence.binding_id : null;
  const canEdit = relation === null || relation === undefined || configuredRelation !== null;
  const selectedKind = getManualRelationKindOption(kind);
  const sourceLabel = resourceLabel(sourceResource, sourceId);
  const targetLabel = resourceLabel(targetResource, targetId);
  const controlsDisabled = pending !== "idle" || !canEdit;
  const formInvalid = !sourceId || !targetId || sourceId === targetId || selectedKind === null;

  useEffect(() => {
    const nextSourceId = relation?.source_resource_id ?? source?.resource_id ?? "";
    setSourceId(nextSourceId);
    setSourceResource(
      source !== undefined && source !== null && source.resource_id === nextSourceId
        ? source
        : null,
    );
    setTargetId(endpointId(relation, "target"));
    setTargetResource(null);
    setKind(relation?.kind ?? MANUAL_RELATION_KIND_OPTIONS[0].id);
    setError(null);
  }, [relation?.relation_id, relation?.kind, relation?.source_resource_id, relation?.target_resource_id, source?.resource_id]);

  function selectSource(resource: ResourceDto) {
    setSourceId(resource.resource_id);
    setSourceResource(resource);
    setError(null);
  }

  function selectTarget(resource: ResourceDto) {
    setTargetId(resource.resource_id);
    setTargetResource(resource);
    setError(null);
  }

  function swapDirection() {
    if (controlsDisabled) return;
    setSourceId(targetId);
    setTargetId(sourceId);
    setSourceResource(targetResource);
    setTargetResource(sourceResource);
    setError(null);
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (formInvalid || selectedKind === null) {
      setError(
        sourceId === targetId
          ? "来源和目标必须是两个不同的资源。"
          : "请选择来源、关系类型和目标资源。",
      );
      return;
    }
    setError(null);
    setPending("saving");
    try {
      if (configuredBindingId !== null) {
        await adapter.updateBinding({
          binding_id: configuredBindingId,
          source_resource_id: sourceId,
          target_resource_id: targetId,
          kind: selectedKind.id,
        });
      } else {
        await adapter.createBinding({
          source_resource_id: sourceId,
          target_resource_id: targetId,
          kind: selectedKind.id,
        });
      }
      onSaved({
        action: configuredBindingId === null ? "created" : "updated",
        sourceResourceId: sourceId,
        targetResourceId: targetId,
        kind: selectedKind.id,
      });
    } catch (caught) {
      const code = desktopErrorCode(caught);
      if (code === "binding_conflict") {
        onSaved({
          action: "existing",
          sourceResourceId: sourceId,
          targetResourceId: targetId,
          kind: selectedKind.id,
        });
        return;
      }
      const message = code === "binding_not_found"
        ? "要编辑的本地关系已不存在，请刷新拓扑后重试。"
        : code === "binding_temporal_conflict"
          ? "关系刚刚发生变化，请刷新拓扑后重试。"
        : code === "invalid_binding"
          ? "关系参数无效，请重新选择来源、类型和目标。"
          : `无法${configuredRelation === null ? "创建" : "更新"}本地关系（${code}）。`;
      setError(message);
    } finally {
      setPending("idle");
    }
  }

  async function disable() {
    if (configuredBindingId === null || controlsDisabled) return;
    setError(null);
    setPending("disabling");
    try {
      await adapter.disableBinding({ binding_id: configuredBindingId });
      onSaved({
        action: "disabled",
        sourceResourceId: sourceId,
        targetResourceId: targetId,
        kind,
      });
    } catch (caught) {
      setError(`无法禁用本地关系（${desktopErrorCode(caught)}）。`);
    } finally {
      setPending("idle");
    }
  }

  return (
    <form aria-busy={pending !== "idle"} className="relation-builder" onSubmit={save}>
      <header className="relation-builder-header">
        <div>
          <p className="relation-builder-eyebrow">本地关系配置</p>
          <h2>{configuredRelation === null ? "建立本地关系" : "编辑本地关系"}</h2>
        </div>
        <span className="relation-builder-mode">
          {configuredRelation === null ? "CREATE" : "CONFIGURED"}
        </span>
      </header>

      <p className="relation-builder-notice" role="note">
        这是你手工声明的本地关系，未通过 Provider 验证，不会执行外部操作。
      </p>

      {relation !== null && relation !== undefined && configuredRelation === null ? (
        <p className="relation-builder-state relation-builder-state--error" role="alert">
          只有 configured 关系可以编辑；Provider 或 inferred 关系保持只读。
        </p>
      ) : null}
      {configuredRelation?.lifecycle === "orphaned" ? (
        <p className="relation-builder-state" role="status">
          当前关系含有未解析端点，保存后仍会保留 configured 证据。
        </p>
      ) : null}

      <div className="relation-builder-fields">
        <ResourcePicker
          disabled={controlsDisabled}
          label="来源资源"
          onSelect={selectSource}
          selectedId={sourceId}
          selectedResource={sourceResource}
        />
        <button
          aria-label="交换关系方向"
          className="relation-builder-swap"
          disabled={controlsDisabled || !sourceId || !targetId}
          onClick={swapDirection}
          type="button"
        >
          ⇄
        </button>
        <ResourcePicker
          disabled={controlsDisabled}
          label="目标资源"
          onSelect={selectTarget}
          selectedId={targetId}
          selectedResource={targetResource}
        />
      </div>

      <fieldset className="relation-builder-kind-field" disabled={controlsDisabled}>
        <legend>关系类型</legend>
        <div aria-label="关系类型" className="relation-builder-kind-list" role="listbox">
          {MANUAL_RELATION_KIND_OPTIONS.map((option) => (
            <button
              aria-selected={option.id === kind}
              className={option.id === kind ? "is-selected" : ""}
              key={option.id}
              onClick={() => {
                setKind(option.id as ManualRelationKind);
                setError(null);
              }}
              role="option"
              type="button"
            >
              <strong>{option.label}</strong>
              <span>{option.id}</span>
              <small>{option.sourceHint} → {option.targetHint}</small>
            </button>
          ))}
        </div>
      </fieldset>

      <div className="relation-builder-preview-block">
        <span className="relation-builder-label">方向预览</span>
        <p aria-live="polite" className="relation-builder-preview">
          {selectedKind === null
            ? "请选择冻结的关系类型。"
            : `${sourceLabel} → ${selectedKind.label} → ${targetLabel}`}
        </p>
        {selectedKind !== null ? (
          <code>{selectedKind.id}</code>
        ) : null}
      </div>

      {error !== null ? (
        <p className="relation-builder-state relation-builder-state--error" role="alert">
          {error}
        </p>
      ) : null}
      {pending !== "idle" ? (
        <p aria-live="polite" className="relation-builder-state" role="status">
          {pending === "saving" ? "正在保存本地关系…" : "正在禁用本地关系…"}
        </p>
      ) : null}

      <footer className="relation-builder-actions">
        {configuredRelation !== null ? (
          <button
            className="relation-builder-danger"
            disabled={controlsDisabled}
            onClick={disable}
            type="button"
          >
            禁用关系
          </button>
        ) : null}
        <span className="relation-builder-actions-spacer" />
        <button disabled={pending !== "idle"} onClick={onCancel} type="button">
          取消
        </button>
        <button className="relation-builder-primary" disabled={controlsDisabled} type="submit">
          {configuredRelation === null ? "保存关联" : "保存修改"}
        </button>
      </footer>
    </form>
  );
}
