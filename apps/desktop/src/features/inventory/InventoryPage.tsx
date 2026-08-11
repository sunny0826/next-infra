import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ConnectionPurgeSummary } from "../../generated/query/ConnectionPurgeSummary";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { canPurgeConnection } from "../../platform/desktop-adapter/connection-capabilities";
import type { DesktopAdapter } from "../../platform/desktop-adapter/desktop-adapter";
import {
  matchesStatusFilter,
  summarizeConnectionExpiry,
  type InventoryStatusFilter,
} from "./inventory-expiry";
import {
  buildResourceForest,
  flattenVisibleCount,
  type ResourceTreeNode,
} from "./inventory-tree";

import "./inventory.css";

interface InventoryPageProps {
  readonly onSelectResource?: (resource: ResourceDto) => void;
  readonly queryVersion?: number;
}

export function compactResourceId(resourceId: string, maxLength = 36): string {
  if (resourceId.length <= maxLength) return resourceId;
  const tailLength = 12;
  return `${resourceId.slice(0, maxLength - tailLength - 1)}…${resourceId.slice(-tailLength)}`;
}

type InventoryState =
  | { readonly type: "loading" }
  | { readonly type: "error"; readonly message: string }
  | {
      readonly type: "ready";
      readonly items: readonly ResourceDto[];
      readonly nextCursor: string | null;
      /** Null when the relations query failed — degrade to a flat list. */
      readonly relations: readonly RelationDto[] | null;
    };

function selectWithKeyboard(
  event: KeyboardEvent<HTMLTableRowElement>,
  resource: ResourceDto,
  onSelectResource?: (resource: ResourceDto) => void,
) {
  if (event.target !== event.currentTarget) return;
  if (event.key !== "Enter" && event.key !== " ") return;
  event.preventDefault();
  onSelectResource?.(resource);
}

async function loadAllRelations(
  adapter: DesktopAdapter,
  resourceIds: readonly string[],
): Promise<readonly RelationDto[]> {
  if (resourceIds.length === 0) return [];

  const relations: RelationDto[] = [];
  const seenCursors = new Set<string>();
  let cursor: string | undefined;
  let snapshotVersion: string | undefined;

  while (true) {
    const page = await adapter.getRelationsForResources({
      resource_ids: resourceIds,
      limit: 400,
      ...(cursor === undefined ? {} : { cursor }),
    });
    if (snapshotVersion !== undefined && page.metadata.snapshot_version !== snapshotVersion) {
      throw new Error("Relation snapshot changed while loading pages.");
    }
    snapshotVersion = page.metadata.snapshot_version;
    relations.push(...page.items);

    const nextCursor = page.page_info.next_cursor;
    if (nextCursor === null) return relations;
    if (seenCursors.has(nextCursor)) {
      throw new Error("Relation pagination cursor repeated.");
    }
    seenCursors.add(nextCursor);
    cursor = nextCursor;
  }
}

const STATUS_FILTERS: ReadonlyArray<{ id: InventoryStatusFilter; label: string }> = [
  { id: "all", label: "全部" },
  { id: "attention", label: "需关注" },
  { id: "expired", label: "已过期" },
  { id: "removed", label: "已失效" },
];

interface TreeRowProps {
  readonly depth: number;
  readonly node: ResourceTreeNode;
  readonly collapsed: boolean;
  readonly connectionById: Readonly<Record<string, ConnectionDto>>;
  readonly onSelectResource?: (resource: ResourceDto) => void;
  readonly onToggleCollapsed: (resourceId: string) => void;
}

function TreeRow({
  depth,
  node,
  collapsed,
  connectionById,
  onSelectResource,
  onToggleCollapsed,
}: TreeRowProps) {
  const { resource, children } = node;
  const hasChildren = children.length > 0;
  const rowClass =
    resource.lifecycle === "tombstoned"
      ? "inventory-row--tombstoned"
      : resource.lifecycle === "orphaned"
        ? "inventory-row--orphaned"
        : "";
  return (
    <tr
      className={rowClass}
      onClick={() => onSelectResource?.(resource)}
      onKeyDown={(event) => selectWithKeyboard(event, resource, onSelectResource)}
      tabIndex={0}
    >
      <td>
        <span
          className="inventory-tree-line"
          style={{ paddingLeft: `${depth * 16}px` }}
        >
          {hasChildren ? (
            <button
              aria-expanded={!collapsed}
              aria-label={`${collapsed ? "展开" : "收起"} ${resource.display_name}`}
              className="inventory-disclosure"
              onClick={(event) => {
                event.stopPropagation();
                onToggleCollapsed(resource.resource_id);
              }}
              type="button"
            >
              <svg
                aria-hidden="true"
                className="inventory-disclosure-chevron"
                viewBox="0 0 16 16"
              >
                <path
                  d="M4.5 6 8 9.5 11.5 6"
                  fill="none"
                  stroke="currentColor"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="1.5"
                />
              </svg>
            </button>
          ) : (
            <span aria-hidden="true" className="inventory-disclosure-spacer" />
          )}
          <span className="inventory-tree-label">
            <strong>{resource.display_name}</strong>
            <code
              aria-label={resource.resource_id}
              className="inventory-resource-id"
              title={resource.resource_id}
            >
              {compactResourceId(resource.resource_id)}
            </code>
          </span>
        </span>
      </td>
      <td><code>{resource.kind}</code></td>
      <td><code>{resource.scope}</code></td>
      <td><span className={`inventory-badge state-${resource.health}`}>{displayEnum(resource.health)}</span></td>
      <td><span className={`inventory-badge state-${resource.freshness}`}>{displayEnum(resource.freshness)}</span></td>
      <td>{connectionById[resource.connection_id]?.display_name ?? resource.connection_id}</td>
      <td><time dateTime={resource.observed_at}>{resource.observed_at}</time></td>
      <td><span className={`inventory-badge state-${resource.lifecycle}`}>{displayEnum(resource.lifecycle)}</span></td>
    </tr>
  );
}

export function InventoryPage({ onSelectResource, queryVersion = 0 }: InventoryPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<InventoryState>({ type: "loading" });
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<InventoryStatusFilter>("all");
  const [cursor, setCursor] = useState<string | undefined>();
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [connectionById, setConnectionById] = useState<Readonly<Record<string, ConnectionDto>>>({});
  const [reloadKey, setReloadKey] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [purgeTarget, setPurgeTarget] = useState<{
    readonly connection: ConnectionDto;
    readonly summary: ConnectionPurgeSummary;
  } | null>(null);
  const [purging, setPurging] = useState(false);

  useEffect(() => {
    let active = true;
    setState({ type: "loading" });
    setCollapsed(new Set());
    async function load() {
      const page = await adapter.searchResources({
        query: query || undefined,
        cursor,
        limit: 25,
      });
      let relations: readonly RelationDto[] | null = null;
      try {
        relations = await loadAllRelations(
          adapter,
          page.items.map((item) => item.resource_id),
        );
      } catch {
        // Relations are an enhancement; the page degrades to the flat list.
      }
      let connections: readonly ConnectionDto[] = [];
      try {
        const connectionPage = await adapter.listConnections();
        connections = connectionPage.items;
      } catch {
        // Connection names are an enhancement; the page falls back to raw ids.
      }
      if (!active) return;
      setConnectionById(
        Object.fromEntries(connections.map((connection) => [connection.connection_id, connection])),
      );
      setState({
        type: "ready",
        items: page.items,
        nextCursor: page.page_info.next_cursor,
        relations,
      });
    }
    load().catch(() => {
      if (active) {
        setState({
          type: "error",
          message: "无法完成受限资源清单查询。",
        });
      }
    });
    return () => {
      active = false;
    };
  }, [adapter, cursor, query, queryVersion, reloadKey]);

  const visibleItems = useMemo(() => {
    if (state.type !== "ready") return [];
    return state.items.filter((resource) => matchesStatusFilter(resource, statusFilter));
  }, [statusFilter, state]);

  const forest = useMemo(() => {
    if (state.type !== "ready") return [];
    return buildResourceForest(visibleItems, state.relations ?? []);
  }, [state, visibleItems]);

  const expiryRows = useMemo(() => {
    if (state.type !== "ready") return [];
    return summarizeConnectionExpiry(state.items, Object.values(connectionById)).filter((row) =>
      canPurgeConnection(row.connection.connector_type),
    );
  }, [state, connectionById]);

  const rows = useMemo(() => {
    const flattened: Array<{ node: ResourceTreeNode; depth: number }> = [];
    const walk = (nodes: readonly ResourceTreeNode[], depth: number) => {
      for (const node of nodes) {
        flattened.push({ node, depth });
        if (node.children.length > 0 && !collapsed.has(node.resource.resource_id)) {
          walk(node.children, depth + 1);
        }
      }
    };
    walk(forest, 0);
    return flattened;
  }, [collapsed, forest]);

  function toggleCollapsed(resourceId: string) {
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(resourceId)) next.delete(resourceId);
      else next.add(resourceId);
      return next;
    });
  }

  async function beginPurge(connection: ConnectionDto) {
    if (!canPurgeConnection(connection.connector_type)) return;
    setNotice(null);
    try {
      const summary = await adapter.previewConnectionPurge(connection.connection_id);
      setPurgeTarget({ connection, summary });
    } catch {
      setNotice("无法读取该连接的本地快照清理范围。");
    }
  }

  async function confirmPurge() {
    if (purgeTarget === null || purging) return;
    setPurging(true);
    try {
      await adapter.purgeConnection(purgeTarget.connection.connection_id);
      setPurgeTarget(null);
      setReloadKey((value) => value + 1);
    } catch {
      setNotice("无法删除该连接的本地快照；现有数据已保留。");
    } finally {
      setPurging(false);
    }
  }

  return (
    <div className="inventory-page">
      <header className="inventory-header">
        <div>
          <p className="inventory-eyebrow">受限本地投影</p>
          <h1>资源清单</h1>
          <p>在浏览器中筛选当前资源，不重新计算提供方状态。</p>
        </div>
        <span>每页 25 条 · 最多 100 条</span>
      </header>

      <div className="inventory-filters" role="search">
        <label>
          <span>资源筛选</span>
          <input
            onChange={(event) => {
              setCursor(undefined);
              setQuery(event.currentTarget.value);
            }}
            placeholder="名称、类型或本地标识"
            type="search"
            value={query}
          />
        </label>
        <div className="inventory-status-group" role="group" aria-label="状态筛选">
          {STATUS_FILTERS.map((filter) => (
            <button
              aria-pressed={statusFilter === filter.id}
              className={statusFilter === filter.id ? "is-active" : ""}
              key={filter.id}
              onClick={() => setStatusFilter(filter.id)}
              type="button"
            >
              {filter.label}
            </button>
          ))}
        </div>
      </div>

      {state.type === "loading" ? (
        <section className="inventory-state" aria-busy="true">
          正在读取资源页…
        </section>
      ) : null}
      {state.type === "error" ? (
        <section className="inventory-state inventory-state--error" role="alert">
          {state.message}
        </section>
      ) : null}
      {state.type === "ready" ? (
        <>
          {notice !== null ? (
            <div className="inventory-notice" role="alert">
              {notice}
            </div>
          ) : null}
          {expiryRows.length > 0 ? (
            <section className="inventory-expiry" aria-label="过期数据清理">
              <header>
                <strong>过期数据</strong>
                <span>按连接清理本地投影；将同时删除该连接的凭据。</span>
              </header>
              {expiryRows.map((row) => (
                <div className="inventory-expiry-row" key={row.connection.connection_id}>
                  <span className="inventory-expiry-name">{row.connection.display_name}</span>
                  <span className="inventory-expiry-counts">
                    {row.expiredCount} 已过期 · {row.removedCount} 已失效
                  </span>
                  <button type="button" onClick={() => beginPurge(row.connection)}>
                    清理本地数据
                  </button>
                </div>
              ))}
            </section>
          ) : null}
          {purgeTarget !== null ? (
            <section className="inventory-purge-confirmation" role="alert">
              <div>
                <h2>删除本地快照</h2>
                <p>将永久删除“{purgeTarget.connection.display_name}”的本地数据与凭据，无法恢复。</p>
              </div>
              <dl>
                <div><dt>资源</dt><dd>{purgeTarget.summary.resources}</dd></div>
                <div><dt>关系</dt><dd>{purgeTarget.summary.relations}</dd></div>
                <div><dt>资源版本</dt><dd>{purgeTarget.summary.resource_versions}</dd></div>
                <div><dt>关系版本</dt><dd>{purgeTarget.summary.relation_versions}</dd></div>
                <div><dt>变更</dt><dd>{purgeTarget.summary.changes}</dd></div>
                <div><dt>绑定</dt><dd>{purgeTarget.summary.bindings}</dd></div>
                <div><dt>同步记录</dt><dd>{purgeTarget.summary.sync_runs}</dd></div>
              </dl>
              {purgeTarget.summary.bindings > 0 ? (
                <p>包含关联到该连接资源的手工绑定；这些绑定也会被删除。</p>
              ) : null}
              <div className="inventory-purge-actions">
                <button disabled={purging} onClick={() => setPurgeTarget(null)} type="button">
                  取消
                </button>
                <button disabled={purging} onClick={confirmPurge} type="button">
                  {purging ? "正在删除…" : "确认删除本地快照"}
                </button>
              </div>
            </section>
          ) : null}
          <div className="inventory-summary">
            <span>{flattenVisibleCount(forest)} 个可见资源</span>
            <span>健康度、新鲜度和生命周期相互独立</span>
          </div>
          <div className="inventory-frame">
            <table>
              <thead>
                <tr>
                  <th>名称</th><th>类型</th><th>范围</th><th>健康度</th><th>新鲜度</th>
                  <th>连接</th><th>观测时间</th><th>生命周期</th>
                </tr>
              </thead>
              <tbody>
                {rows.map(({ node, depth }) => (
                  <TreeRow
                    key={node.resource.resource_id}
                    collapsed={collapsed.has(node.resource.resource_id)}
                    connectionById={connectionById}
                    depth={depth}
                    node={node}
                    onSelectResource={onSelectResource}
                    onToggleCollapsed={toggleCollapsed}
                  />
                ))}
              </tbody>
            </table>
            {rows.length === 0 ? (
              <div className="inventory-empty">没有资源匹配当前受限筛选条件。</div>
            ) : null}
          </div>
          <nav className="inventory-pagination" aria-label="资源清单分页">
            <button disabled={cursor === undefined} onClick={() => setCursor(undefined)} type="button">
              首页
            </button>
            <button
              disabled={state.nextCursor === null}
              onClick={() => setCursor(state.nextCursor ?? undefined)}
              type="button"
            >
              下一页
            </button>
          </nav>
        </>
      ) : null}
    </div>
  );
}
