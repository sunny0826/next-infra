import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { ResourceDto } from "../../generated/query/ResourceDto";
import { displayEnum } from "../../i18n";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./inventory.css";

interface InventoryPageProps {
  readonly onSelectResource?: (resource: ResourceDto) => void;
  readonly queryVersion?: number;
}

type InventoryState =
  | { readonly type: "loading" }
  | { readonly type: "error"; readonly message: string }
  | {
      readonly type: "ready";
      readonly items: readonly ResourceDto[];
      readonly nextCursor: string | null;
    };

function needsAttention(resource: ResourceDto): boolean {
  return (
    resource.health !== "healthy" ||
    resource.freshness !== "fresh" ||
    resource.lifecycle !== "active"
  );
}

export function InventoryPage({ onSelectResource, queryVersion = 0 }: InventoryPageProps) {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<InventoryState>({ type: "loading" });
  const [query, setQuery] = useState("");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [cursor, setCursor] = useState<string | undefined>();

  useEffect(() => {
    let active = true;
    setState({ type: "loading" });
    adapter
      .searchResources({ query: query || undefined, cursor, limit: 25 })
      .then((page) => {
        if (!active) return;
        setState({
          type: "ready",
          items: page.items,
          nextCursor: page.page_info.next_cursor,
        });
      })
      .catch(() => {
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
  }, [adapter, cursor, query, queryVersion]);

  const visibleItems = useMemo(() => {
    if (state.type !== "ready") return [];
    return [...state.items]
      .filter((resource) => !attentionOnly || needsAttention(resource))
      .sort((left, right) =>
        left.display_name.localeCompare(right.display_name, "en"),
      );
  }, [attentionOnly, state]);

  function selectWithKeyboard(
    event: KeyboardEvent<HTMLTableRowElement>,
    resource: ResourceDto,
  ) {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onSelectResource?.(resource);
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
        <button
          aria-pressed={attentionOnly}
          className={attentionOnly ? "is-active" : ""}
          onClick={() => setAttentionOnly((value) => !value)}
          type="button"
        >
          仅显示需关注项
        </button>
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
          <div className="inventory-summary">
            <span>{visibleItems.length} 个可见资源</span>
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
                {visibleItems.map((resource) => (
                  <tr
                    key={resource.resource_id}
                    onClick={() => onSelectResource?.(resource)}
                    onKeyDown={(event) => selectWithKeyboard(event, resource)}
                    tabIndex={0}
                  >
                    <td><strong>{resource.display_name}</strong><code>{resource.resource_id}</code></td>
                    <td><code>{resource.kind}</code></td>
                    <td><code>{resource.scope}</code></td>
                    <td><span className={`inventory-badge state-${resource.health}`}>{displayEnum(resource.health)}</span></td>
                    <td><span className={`inventory-badge state-${resource.freshness}`}>{displayEnum(resource.freshness)}</span></td>
                    <td><code>{resource.connection_id}</code></td>
                    <td><time dateTime={resource.observed_at}>{resource.observed_at}</time></td>
                    <td><span className={`inventory-badge state-${resource.lifecycle}`}>{displayEnum(resource.lifecycle)}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
            {visibleItems.length === 0 ? (
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
