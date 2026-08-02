export function RuntimeBar() {
  return (
    <footer aria-label="Control Plane Runtime" className="shell-runtime-bar">
      <div className="shell-runtime-cluster">
        <span className="shell-runtime-core shell-status shell-status-unknown">
          <span className="shell-status-dot" /> Runtime not connected
        </span>
        <span>local · read-only</span>
      </div>
      <div className="shell-runtime-cluster shell-runtime-secondary">
        <span>Goal 1 shell</span>
        <span>no provider access</span>
      </div>
    </footer>
  );
}
