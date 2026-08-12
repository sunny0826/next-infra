import { useEffect, useRef, type MouseEvent } from "react";
import { createPortal } from "react-dom";

import type { RelationDto } from "../../../generated/query/RelationDto";
import type { ResourceDto } from "../../../generated/query/ResourceDto";
import { Icon } from "../../../ui/Icon";

import { RelationBuilder, type RelationMutationResult } from "./RelationBuilder";

export interface RelationDialogProps {
  readonly relation: RelationDto | null;
  readonly source: ResourceDto | null;
  readonly onClose: () => void;
  readonly onSaved: (result: RelationMutationResult) => void;
}

const FOCUSABLE_SELECTOR = [
  "button:not(:disabled)",
  "input:not(:disabled)",
  "[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function RelationDialog({ relation, source, onClose, onSaved }: RelationDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    closeButtonRef.current?.focus();

    function handleKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
      if (focusable === undefined || focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.body.style.overflow = previousOverflow;
      previousFocus?.focus();
    };
  }, [onClose]);

  function closeFromBackdrop(event: MouseEvent<HTMLDivElement>) {
    if (event.target === event.currentTarget) onClose();
  }

  return createPortal(
    <div className="relation-dialog-overlay" onMouseDown={closeFromBackdrop} role="presentation">
      <div
        aria-label="资源关系配置"
        aria-modal="true"
        className="relation-dialog"
        ref={dialogRef}
        role="dialog"
      >
        <header className="relation-dialog-header">
          <div>
            <p>TOPOLOGY RELATION</p>
            <h2>关系配置工作台</h2>
            <span>建立资源之间有方向、可追溯的本地关系</span>
          </div>
          <button
            aria-label="关闭关系配置"
            className="relation-dialog-close"
            onClick={onClose}
            ref={closeButtonRef}
            type="button"
          >
            <Icon name="close" />
          </button>
        </header>
        <div className="relation-dialog-body">
          <RelationBuilder
            onCancel={onClose}
            onSaved={onSaved}
            relation={relation}
            source={source}
          />
        </div>
      </div>
    </div>,
    document.body,
  );
}
