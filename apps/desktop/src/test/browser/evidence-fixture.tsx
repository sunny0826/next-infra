import { StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import { EvidenceCard } from "../../features/evidence/EvidenceCard";
import { EvidenceSpine } from "../../features/evidence/EvidenceSpine";
import { DesktopAdapterProvider } from "../../platform/desktop-adapter/DesktopAdapterContext";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../fixtures/query-fixtures";
import { InspectorHost, type InspectorSelection } from "../../ui/InspectorHost";
import "../../styles/shell.css";
import "./evidence-fixture.css";

/**
 * Browser-only preview of the Evidence Spine and the Evidence Inspector.
 *
 * All data is synthetic fixture content (fixture-* prefix); timestamps are
 * derived from the load time so relative labels read naturally. No real
 * repository, host, address, or credential is referenced. This page is a
 * visual QA surface, not a product route.
 */

function minutesAgo(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

function daysAgo(days: number): string {
  return new Date(Date.now() - days * 86_400_000).toISOString();
}

const SOURCE: ResourceDto = {
  resource_id: "fixture-resource-alpha",
  connection_id: "fixture-connection-alpha",
  kind: "fixture.compute.node",
  display_name: "Fixture Compute Alpha",
  scope: "fixture-scope",
  lifecycle: "active",
  health: "healthy",
  freshness: "fresh",
  observed_at: minutesAgo(2),
};

const TARGET: ResourceDto = {
  resource_id: "fixture-resource-beta",
  connection_id: "fixture-connection-alpha",
  kind: "fixture.database.instance",
  display_name: "Fixture Database Beta",
  scope: "fixture-scope",
  lifecycle: "active",
  health: "healthy",
  freshness: "expired",
  observed_at: minutesAgo(5),
};

const RELATIONS: readonly RelationDto[] = [
  {
    relation_id: "fixture-relation-provider-alpha-beta",
    source_resource_id: SOURCE.resource_id,
    target_resource_id: TARGET.resource_id,
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: "fixture",
      connection_id: "fixture-connection-alpha",
      sync_run_id: "fixture-sync-run-2026-08-10-0742",
      field_path: "attributes.target",
    },
    last_seen_at: minutesAgo(18),
  },
  {
    relation_id: "fixture-relation-configured-alpha-beta",
    source_resource_id: SOURCE.resource_id,
    target_resource_id: TARGET.resource_id,
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "configured",
    evidence: {
      type: "configured",
      binding_id: "fixture-binding-alpha-beta",
      created_at: daysAgo(4),
    },
    last_seen_at: minutesAgo(2),
  },
  {
    relation_id: "fixture-relation-inferred-alpha-beta",
    source_resource_id: SOURCE.resource_id,
    target_resource_id: TARGET.resource_id,
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "inferred",
    evidence: {
      type: "inferred",
      rule_version: "fixture-rule-v1",
      input_resource_version_ids: [
        "fixture-resource-version-alpha-2026-08-10-0730",
        "fixture-resource-version-beta-2026-08-10-0740",
      ],
      input_relation_version_ids: ["fixture-relation-version-alpha-2026-08-10-0735"],
      confidence_basis_points: 9200,
    },
    last_seen_at: minutesAgo(2),
  },
];

/** Long (>24 chars) connection id so the copy button is visible in screenshots. */
const OPEN_CARD_RELATION: RelationDto = {
  relation_id: "fixture-relation-provider-alpha-beta",
  source_resource_id: SOURCE.resource_id,
  target_resource_id: TARGET.resource_id,
  kind: "fixture.depends_on",
  lifecycle: "active",
  evidence_type: "provider",
  evidence: {
    type: "provider",
    connector_type: "fixture",
    connection_id: "fixture-connection-very-long-identifier-0001",
    sync_run_id: "fixture-sync-run-2026-08-10-0742",
    field_path: "attributes.target",
  },
  last_seen_at: minutesAgo(18),
};

/** Six rows (>5) with unique ids so the expand toggle is exercised. */
const LONG_RELATIONS: readonly RelationDto[] = [
  ...RELATIONS,
  ...RELATIONS.map((relation) => ({
    ...relation,
    relation_id: `${relation.relation_id}-dup`,
  })),
];

function InspectorDemo({
  label,
  selection,
}: {
  readonly label: string;
  readonly selection: InspectorSelection;
}) {
  const [open, setOpen] = useState(true);
  const asideHeadRef = useRef<HTMLDivElement>(null);
  return (
    <div className="preview-inspector">
      <p className="preview-label">{label}</p>
      <DesktopAdapterProvider
        adapter={new MockDesktopAdapter(createQueryEvidenceLifecycleSnapshotFixture())}
      >
        <InspectorHost
          asideHeadRef={asideHeadRef}
          onClose={() => setOpen(false)}
          onCreateRelation={() => undefined}
          onEditRelation={() => undefined}
          open={open}
          routeLabel="Fixture 拓扑"
          selection={selection}
        />
      </DesktopAdapterProvider>
    </div>
  );
}

const container = document.getElementById("root");

if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <div className="preview-shell">
        <main className="preview-canvas">
          <p className="preview-label">资源详情 · 证据链（提供方 / 已配置 / 推断）</p>
          <EvidenceSpine
            source={SOURCE}
            target={TARGET}
            relations={RELATIONS}
          />
          <p className="preview-label">展开态证据卡（defaultOpen · 长 ID 复制按钮）</p>
          <div className="preview-open-card">
            <EvidenceCard relation={OPEN_CARD_RELATION} defaultOpen />
          </div>
          <p className="preview-label">无证据空态</p>
          <EvidenceSpine source={SOURCE} target={TARGET} relations={[]} />
          <p className="preview-label">长证据路径（折叠态）</p>
          <EvidenceSpine source={SOURCE} target={TARGET} relations={LONG_RELATIONS} />
        </main>
        <aside className="preview-side">
          <InspectorDemo
            label="检查器 · 推断关系（置信度条 + 折叠详情）"
            selection={{ type: "relation", relation: RELATIONS[2] }}
          />
          <InspectorDemo
            label="检查器 · 配置关系（编辑按钮）"
            selection={{ type: "relation", relation: RELATIONS[1] }}
          />
          <InspectorDemo
            label="检查器 · 资源选择（信息 callout）"
            selection={{ type: "resource", resource: SOURCE }}
          />
        </aside>
      </div>
    </StrictMode>,
  );
}
