import type { RefObject } from "react";

import type { RouteId } from "../app/routes";
import { Icon } from "./Icon";

interface ContextBarProps {
  contextLabel: string;
  routeId: RouteId;
  searchInputRef: RefObject<HTMLInputElement | null>;
}

export function ContextBar({ contextLabel, routeId, searchInputRef }: ContextBarProps) {
  return (
    <header className="shell-context-bar">
      <div className="shell-context-scope">
        <Icon name={routeId} />
        <strong>{contextLabel}</strong>
      </div>

      <label className="shell-search" htmlFor="shell-search-input">
        <span className="visually-hidden">Search local infrastructure</span>
        <Icon name="search" />
        <input
          aria-keyshortcuts="Meta+K Control+K"
          autoComplete="off"
          id="shell-search-input"
          placeholder="Search becomes available with the Query adapter"
          readOnly
          ref={searchInputRef}
          type="search"
        />
        <span aria-hidden="true" className="shell-shortcut">
          ⌘ K
        </span>
      </label>

      <div className="shell-context-status" aria-label="Snapshot context">
        <span>local · read-only</span>
        <span className="shell-status shell-status-unknown">
          <span className="shell-status-dot" /> Goal 1 shell
        </span>
      </div>
    </header>
  );
}
