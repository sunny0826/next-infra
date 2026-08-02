import type { ShellRoute } from "../app/routes";

interface PrimaryCanvasProps {
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  route: ShellRoute;
}

export function PrimaryCanvas({ inspectorOpen, onOpenInspector, route }: PrimaryCanvasProps) {
  return (
    <main className="shell-primary-canvas" id="primary-canvas" tabIndex={-1}>
      <div className="shell-route-page">
        <header className="shell-route-header">
          <div className="shell-route-title">
            <p className="shell-eyebrow">Local query surface</p>
            <h1>{route.label}</h1>
            <p>{route.description}</p>
          </div>
          {!inspectorOpen ? (
            <button className="shell-control-button" onClick={onOpenInspector} type="button">
              Open inspector
            </button>
          ) : null}
        </header>

        <section aria-labelledby="goal-one-placeholder" className="shell-placeholder">
          <div>
            <h2 id="goal-one-placeholder">Goal 1 placeholder</h2>
            <p>This route has no query data or external operations.</p>
          </div>
        </section>
      </div>
    </main>
  );
}
