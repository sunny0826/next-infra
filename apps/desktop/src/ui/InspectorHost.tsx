import { Icon } from "./Icon";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";

export type InspectorSelection =
  | { readonly type: "resource"; readonly resource: ResourceDto }
  | { readonly type: "relation"; readonly relation: RelationDto }
  | null;

interface InspectorHostProps {
  onClose: () => void;
  open: boolean;
  routeLabel: string;
  selection: InspectorSelection;
}

function RelationEvidence({ relation }: { relation: RelationDto }) {
  const evidence = relation.evidence;

  if (evidence.type === "provider") {
    return (
      <dl className="shell-inspector-facts">
        <dt>Evidence type</dt><dd>provider</dd>
        <dt>Connector</dt><dd><code>{evidence.connector_type}</code></dd>
        <dt>Connection</dt><dd><code>{evidence.connection_id}</code></dd>
        <dt>SyncRun</dt><dd><code>{evidence.sync_run_id}</code></dd>
        <dt>Field path</dt><dd><code>{evidence.field_path}</code></dd>
      </dl>
    );
  }

  if (evidence.type === "configured") {
    return (
      <dl className="shell-inspector-facts">
        <dt>Evidence type</dt><dd>configured</dd>
        <dt>Binding</dt><dd><code>{evidence.binding_id}</code></dd>
        <dt>Created</dt><dd><time dateTime={evidence.created_at}>{evidence.created_at}</time></dd>
      </dl>
    );
  }

  return (
    <dl className="shell-inspector-facts">
      <dt>Evidence type</dt><dd>inferred</dd>
      <dt>Rule version</dt><dd><code>{evidence.rule_version}</code></dd>
      <dt>Resource inputs</dt>
      <dd>
        <ul>
          {evidence.input_resource_version_ids.map((versionId) => <li key={versionId}><code>{versionId}</code></li>)}
        </ul>
      </dd>
      <dt>Relation inputs</dt>
      <dd>
        {evidence.input_relation_version_ids.length === 0 ? "None" : (
          <ul>
            {evidence.input_relation_version_ids.map((versionId) => <li key={versionId}><code>{versionId}</code></li>)}
          </ul>
        )}
      </dd>
      <dt>Confidence</dt><dd>{evidence.confidence_basis_points} bp</dd>
    </dl>
  );
}

export function InspectorHost({ onClose, open, routeLabel, selection }: InspectorHostProps) {
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
        {selection === null ? (
          <>
            <p className="shell-inspector-type">No selection</p>
            <h3>Evidence Spine</h3>
            <p className="shell-inspector-subtitle">Select a resource or relation</p>
          </>
        ) : null}

        {selection?.type === "resource" ? (
          <>
            <p className="shell-inspector-type">Resource</p>
            <h3>{selection.resource.display_name}</h3>
            <code>{selection.resource.resource_id}</code>
            <dl className="shell-inspector-facts">
              <dt>Connection</dt><dd><code>{selection.resource.connection_id}</code></dd>
              <dt>Scope</dt><dd>{selection.resource.scope}</dd>
            </dl>
            <h4>Current Facts</h4>
            <dl className="shell-inspector-facts">
              <dt>Health</dt><dd>{selection.resource.health}</dd>
              <dt>Freshness</dt><dd>{selection.resource.freshness}</dd>
              <dt>Lifecycle</dt><dd>{selection.resource.lifecycle}</dd>
              <dt>Observed</dt><dd><time dateTime={selection.resource.observed_at}>{selection.resource.observed_at}</time></dd>
            </dl>
            <h4>Evidence</h4>
            <p className="shell-inspector-subtitle">ResourceDto exposes current facts only; relation provenance is not present on this selection.</p>
          </>
        ) : null}

        {selection?.type === "relation" ? (
          <>
            <p className="shell-inspector-type">Relation</p>
            <h3>{selection.relation.kind}</h3>
            <code>{selection.relation.relation_id}</code>
            <h4>Current Facts</h4>
            <dl className="shell-inspector-facts">
              <dt>Lifecycle</dt><dd>{selection.relation.lifecycle}</dd>
              <dt>Source</dt><dd><code>{selection.relation.source_resource_id}</code></dd>
              <dt>Target</dt><dd><code>{selection.relation.target_resource_id}</code></dd>
              <dt>Last seen</dt><dd><time dateTime={selection.relation.last_seen_at}>{selection.relation.last_seen_at}</time></dd>
            </dl>
            <h4>Evidence</h4>
            <RelationEvidence relation={selection.relation} />
          </>
        ) : null}
      </div>
    </aside>
  );
}
