import type { RefObject } from "react";
import type { ResourceDto } from "../generated/query/ResourceDto";

import type { RouteId } from "../app/routes";
import { Icon } from "./Icon";

interface ContextBarProps {
  contextLabel: string;
  routeId: RouteId;
  searchInputRef: RefObject<HTMLInputElement | null>;
  searchResults: readonly ResourceDto[];
  searchValue: string;
  onSearchChange: (value: string) => void;
  onSearchClear: () => void;
  onSearchSelect: (resource: ResourceDto) => void;
}

export function ContextBar({ contextLabel, routeId, searchInputRef, searchResults, searchValue, onSearchChange, onSearchClear, onSearchSelect }: ContextBarProps) {
  const searchResultsId = "shell-search-results";
  const searchOpen = searchResults.length > 0;

  return (
    <header className="shell-context-bar">
      <div className="shell-context-scope">
        <Icon name={routeId} />
        <strong>{contextLabel}</strong>
      </div>

      <div className="shell-search-wrap">
        <label className="shell-search" htmlFor="shell-search-input">
          <span className="visually-hidden">Search local infrastructure</span>
          <Icon name="search" />
          <input
            aria-autocomplete="list"
            aria-controls={searchOpen ? searchResultsId : undefined}
            aria-expanded={searchOpen}
            aria-haspopup="listbox"
            aria-keyshortcuts="Meta+K Control+K"
            autoComplete="off"
            id="shell-search-input"
            onChange={(event) => onSearchChange(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              event.preventDefault();
              onSearchClear();
              event.currentTarget.blur();
            }}
            placeholder="Search bounded local resources"
            ref={searchInputRef}
            role="combobox"
            type="search"
            value={searchValue}
          />
          <span aria-hidden="true" className="shell-shortcut">
            ⌘ K
          </span>
        </label>
        {searchOpen ? (
          <div
            aria-label="Search results"
            className="shell-search-results"
            id={searchResultsId}
            role="listbox"
          >
            {searchResults.map((resource) => (
              <button
                aria-selected={false}
                key={resource.resource_id}
                onClick={() => onSearchSelect(resource)}
                role="option"
                type="button"
              >
                <strong>{resource.display_name}</strong>
                <code>{resource.kind}</code>
              </button>
            ))}
          </div>
        ) : null}
      </div>

      <div className="shell-context-status" aria-label="Snapshot context">
        <span>local · read-only</span>
        <span className="shell-status shell-status-unknown">
          <span className="shell-status-dot" /> Query adapter
        </span>
      </div>
    </header>
  );
}
