import { Icon } from "./Icon";

interface InspectorHostProps {
  onClose: () => void;
  open: boolean;
  routeLabel: string;
}

export function InspectorHost({ onClose, open, routeLabel }: InspectorHostProps) {
  return (
    <aside aria-label="Evidence inspector" className="shell-inspector" hidden={!open}>
      <div className="shell-inspector-head">
        <h2>Evidence Spine</h2>
        <button aria-label="Close inspector" className="shell-icon-button" onClick={onClose} type="button">
          <Icon name="close" />
        </button>
      </div>

      <div className="shell-inspector-body">
        <p className="shell-inspector-kicker">{routeLabel} context</p>
        <h3>No selection</h3>
        <p className="shell-inspector-subtitle">Inspector host · Goal 1</p>
        <p className="shell-inspector-summary">
          Selecting a resource, relation, change, or connection will show its evidence here in a later goal.
        </p>
      </div>
    </aside>
  );
}
