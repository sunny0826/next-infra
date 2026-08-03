import { useEffect, useMemo, useState, type KeyboardEvent } from "react";

import type { ResourceDto } from "../../generated/query/ResourceDto";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./inventory.css";

interface InventoryPageProps {
  readonly onSelectResource?: (resource: ResourceDto) => void;
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

export function InventoryPage({ onSelectResource }: InventoryPageProps) {
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
            message: "The bounded inventory query could not be completed.",
          });
        }
      });
    return () => {
      active = false;
    };
  }, [adapter, cursor, query]);

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
          <p className="inventory-eyebrow">Bounded local projection</p>
          <h1>Inventory</h1>
          <p>Filter current resources without recomputing provider state in the browser.</p>
        </div>
        <span>25 per page · maximum 100</span>
      </header>

      <div className="inventory-filters" role="search">
        <label>
          <span>Resource filter</span>
          <input
            onChange={(event) => {
              setCursor(undefined);
              setQuery(event.currentTarget.value);
            }}
            placeholder="Name, kind, or local identity"
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
          Attention only
        </button>
      </div>

      {state.type === "loading" ? (
        <section className="inventory-state" aria-busy="true">
          Reading resource page…
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
            <span>{visibleItems.length} visible resources</span>
            <span>Health, Freshness, and Lifecycle remain independent</span>
          </div>
          <div className="inventory-frame">
            <table>
              <thead>
                <tr>
                  <th>Name</th><th>Kind</th><th>Scope</th><th>Health</th><th>Freshness</th>
                  <th>Connection</th><th>Observed</th><th>Lifecycle</th>
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
                    <td><span className={`inventory-badge state-${resource.health}`}>{resource.health}</span></td>
                    <td><span className={`inventory-badge state-${resource.freshness}`}>{resource.freshness}</span></td>
                    <td><code>{resource.connection_id}</code></td>
                    <td><time dateTime={resource.observed_at}>{resource.observed_at}</time></td>
                    <td><span className={`inventory-badge state-${resource.lifecycle}`}>{resource.lifecycle}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
            {visibleItems.length === 0 ? (
              <div className="inventory-empty">No resources match the current bounded filter.</div>
            ) : null}
          </div>
          <nav className="inventory-pagination" aria-label="Inventory pagination">
            <button disabled={cursor === undefined} onClick={() => setCursor(undefined)} type="button">
              First page
            </button>
            <button
              disabled={state.nextCursor === null}
              onClick={() => setCursor(state.nextCursor ?? undefined)}
              type="button"
            >
              Next page
            </button>
          </nav>
        </>
      ) : null}
    </div>
  );
}
