import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RelationDto } from "../../generated/query/RelationDto";

import { EvidenceCard } from "./EvidenceCard";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const LONG_CONNECTION_ID = "fixture-connection-very-long-identifier-0001";

function providerRelation(connectionId: string): RelationDto {
  return {
    relation_id: "fixture-relation-provider-alpha-beta",
    source_resource_id: "fixture-resource-alpha",
    target_resource_id: "fixture-resource-beta",
    kind: "fixture.depends_on",
    lifecycle: "active",
    evidence_type: "provider",
    evidence: {
      type: "provider",
      connector_type: "fixture",
      connection_id: connectionId,
      sync_run_id: "fixture-sync-run-complete",
      field_path: "attributes.target",
    },
    last_seen_at: "2000-01-01T00:00:00Z",
  };
}

const SHORT_EVIDENCE_RELATION: RelationDto = {
  relation_id: "fixture-relation-short",
  source_resource_id: "fixture-resource-alpha",
  target_resource_id: "fixture-resource-beta",
  kind: "fixture.depends_on",
  lifecycle: "active",
  evidence_type: "provider",
  evidence: {
    type: "provider",
    connector_type: "fixture",
    connection_id: "fixture-connection-a",
    sync_run_id: "fixture-sync-run-a",
    field_path: "attributes.target",
  },
  last_seen_at: "2000-01-01T00:00:00Z",
};

describe("EvidenceCard copy button", () => {
  it("copies the full connection id and swaps the button to 已复制", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });

    render(<EvidenceCard relation={providerRelation(LONG_CONNECTION_ID)} defaultOpen />);

    const copyButton = screen.getByRole("button", { name: `复制 ${LONG_CONNECTION_ID}` });
    fireEvent.click(copyButton);

    await screen.findByText("已复制");
    expect(writeText).toHaveBeenCalledWith(LONG_CONNECTION_ID);
  });

  it("omits the copy button for short identifiers", () => {
    render(<EvidenceCard relation={SHORT_EVIDENCE_RELATION} defaultOpen />);

    expect(screen.queryByRole("button", { name: /复制/ })).not.toBeInTheDocument();
  });
});
