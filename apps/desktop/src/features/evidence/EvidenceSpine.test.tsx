import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";

import { EvidenceSpine } from "./EvidenceSpine";

afterEach(cleanup);

function evidenceFixture() {
  const snapshot = createQueryEvidenceLifecycleSnapshotFixture();
  const source = snapshot.resources.find(
    ({ resource_id }) => resource_id === "fixture-resource-alpha",
  );
  const target = snapshot.resources.find(
    ({ resource_id }) => resource_id === "fixture-resource-beta",
  );

  if (source === undefined || target === undefined) {
    throw new Error("evidence fixture endpoints are missing");
  }

  return { source, target, relations: snapshot.relations };
}

describe("EvidenceSpine", () => {
  it("renders current facts and preserves every evidence row for shared endpoints", () => {
    const fixture = evidenceFixture();
    render(<EvidenceSpine {...fixture} />);

    expect(screen.getByRole("heading", { name: "证据链" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "当前事实" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "证据路径" })).toBeInTheDocument();
    expect(screen.getByLabelText("来源 当前事实")).toHaveTextContent(
      "Fixture Compute Alpha",
    );
    expect(screen.getByLabelText("目标 当前事实")).toHaveTextContent(
      "Fixture Database Beta",
    );

    expect(screen.getAllByLabelText(/ evidence$/)).toHaveLength(3);
    for (const relation of fixture.relations) {
      expect(screen.getByText(relation.relation_id)).toBeInTheDocument();
    }
  });

  it("shows provider provenance without raw provider payloads", () => {
    const fixture = evidenceFixture();
    render(
      <EvidenceSpine
        {...fixture}
        relations={fixture.relations.filter(({ evidence_type }) => evidence_type === "provider")}
      />,
    );

    const row = within(screen.getByLabelText("提供方 evidence"));
    expect(row.getByText("fixture")).toBeInTheDocument();
    expect(row.getByText("fixture-connection-alpha")).toBeInTheDocument();
    expect(row.getByText("fixture-sync-run-complete")).toBeInTheDocument();
    expect(row.getByText("attributes.target")).toBeInTheDocument();
  });

  it("does not invent a SyncRun for configured evidence", () => {
    const fixture = evidenceFixture();
    render(
      <EvidenceSpine
        {...fixture}
        relations={fixture.relations.filter(
          ({ evidence_type }) => evidence_type === "configured",
        )}
      />,
    );

    const row = within(screen.getByLabelText("已配置 evidence"));
    expect(row.getByText("fixture-binding-alpha-beta")).toBeInTheDocument();
    expect(row.getByText("1999-12-31T23:58:00Z")).toBeInTheDocument();
    expect(row.queryByText("SyncRun")).not.toBeInTheDocument();
  });

  it("shows inferred rule, input versions and confidence", () => {
    const fixture = evidenceFixture();
    render(
      <EvidenceSpine
        {...fixture}
        relations={fixture.relations.filter(({ evidence_type }) => evidence_type === "inferred")}
      />,
    );

    const row = within(screen.getByLabelText("推断 evidence"));
    expect(row.getByText("fixture-rule-v1")).toBeInTheDocument();
    expect(row.getByText("fixture-resource-version-alpha")).toBeInTheDocument();
    expect(row.getByText("fixture-relation-version-alpha")).toBeInTheDocument();
    expect(row.getByText(/92%/)).toBeInTheDocument();
    expect(row.getByText(/9200 bp/)).toBeInTheDocument();
  });
});
