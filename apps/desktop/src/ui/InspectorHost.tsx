import { Icon } from "./Icon";
import type { RelationDto } from "../generated/query/RelationDto";
import type { ResourceDto } from "../generated/query/ResourceDto";
import { displayEnum } from "../i18n";

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
        <dt>证据类型</dt><dd>提供方</dd>
        <dt>连接器</dt><dd><code>{evidence.connector_type}</code></dd>
        <dt>连接</dt><dd><code>{evidence.connection_id}</code></dd>
        <dt>SyncRun</dt><dd><code>{evidence.sync_run_id}</code></dd>
        <dt>字段路径</dt><dd><code>{evidence.field_path}</code></dd>
      </dl>
    );
  }

  if (evidence.type === "configured") {
    return (
      <dl className="shell-inspector-facts">
        <dt>证据类型</dt><dd>已配置</dd>
        <dt>绑定</dt><dd><code>{evidence.binding_id}</code></dd>
        <dt>创建时间</dt><dd><time dateTime={evidence.created_at}>{evidence.created_at}</time></dd>
      </dl>
    );
  }

  return (
    <dl className="shell-inspector-facts">
      <dt>证据类型</dt><dd>推断</dd>
      <dt>规则版本</dt><dd><code>{evidence.rule_version}</code></dd>
      <dt>资源输入</dt>
      <dd>
        <ul>
          {evidence.input_resource_version_ids.map((versionId) => <li key={versionId}><code>{versionId}</code></li>)}
        </ul>
      </dd>
      <dt>关系输入</dt>
      <dd>
        {evidence.input_relation_version_ids.length === 0 ? "无" : (
          <ul>
            {evidence.input_relation_version_ids.map((versionId) => <li key={versionId}><code>{versionId}</code></li>)}
          </ul>
        )}
      </dd>
      <dt>置信度</dt><dd>{evidence.confidence_basis_points} bp</dd>
    </dl>
  );
}

export function InspectorHost({ onClose, open, routeLabel, selection }: InspectorHostProps) {
  return (
    <aside aria-label="证据检查器" className="shell-inspector" hidden={!open}>
      <div className="shell-inspector-head">
        <h2>证据链</h2>
        <button aria-label="关闭检查器" className="shell-icon-button" onClick={onClose} type="button">
          <Icon name="close" />
        </button>
      </div>

      <div className="shell-inspector-body">
        <p className="shell-inspector-kicker">{routeLabel} 上下文</p>
        {selection === null ? (
          <>
            <p className="shell-inspector-type">未选择</p>
            <h3>证据链</h3>
            <p className="shell-inspector-subtitle">请选择资源或关系</p>
          </>
        ) : null}

        {selection?.type === "resource" ? (
          <>
            <p className="shell-inspector-type">资源</p>
            <h3>{selection.resource.display_name}</h3>
            <code>{selection.resource.resource_id}</code>
            <dl className="shell-inspector-facts">
              <dt>连接</dt><dd><code>{selection.resource.connection_id}</code></dd>
              <dt>范围</dt><dd>{selection.resource.scope}</dd>
            </dl>
            <h4>当前事实</h4>
            <dl className="shell-inspector-facts">
              <dt>健康度</dt><dd>{displayEnum(selection.resource.health)}</dd>
              <dt>新鲜度</dt><dd>{displayEnum(selection.resource.freshness)}</dd>
              <dt>生命周期</dt><dd>{displayEnum(selection.resource.lifecycle)}</dd>
              <dt>观测时间</dt><dd><time dateTime={selection.resource.observed_at}>{selection.resource.observed_at}</time></dd>
            </dl>
            <h4>证据</h4>
            <p className="shell-inspector-subtitle">ResourceDto 仅提供当前事实；此选择中不包含关系来源。</p>
          </>
        ) : null}

        {selection?.type === "relation" ? (
          <>
            <p className="shell-inspector-type">关系</p>
            <h3>{selection.relation.kind}</h3>
            <code>{selection.relation.relation_id}</code>
            <h4>当前事实</h4>
            <dl className="shell-inspector-facts">
              <dt>生命周期</dt><dd>{displayEnum(selection.relation.lifecycle)}</dd>
              <dt>来源</dt><dd><code>{selection.relation.source_resource_id}</code></dd>
              <dt>目标</dt><dd><code>{selection.relation.target_resource_id}</code></dd>
              <dt>最近观测</dt><dd><time dateTime={selection.relation.last_seen_at}>{selection.relation.last_seen_at}</time></dd>
            </dl>
            <h4>证据</h4>
            <RelationEvidence relation={selection.relation} />
          </>
        ) : null}
      </div>
    </aside>
  );
}
