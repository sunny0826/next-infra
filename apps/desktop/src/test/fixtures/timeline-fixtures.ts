import type { ChangeOriginDto } from "../../generated/query/ChangeOriginDto";
import type { PageInfo } from "../../generated/query/PageInfo";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";
import type { TimelineGroupDto } from "../../generated/query/TimelineGroupDto";
import type { TimelineItemDto } from "../../generated/query/TimelineItemDto";
import type { TimelineOriginDto } from "../../generated/query/TimelineOriginDto";
import type { TimelinePageDto } from "../../generated/query/TimelinePageDto";

import { FIXTURE_OBSERVED_AT } from "./query-fixtures";

/**
 * Synthetic timeline pages. All identifiers stay fixture-scoped and every
 * timestamp is fixed so serialized fixtures are byte-for-byte reproducible.
 * The split-group pair reproduces the historical backend behavior where one
 * logical group is cut by the page boundary; the UI must merge it.
 */
const FIXTURE_GROUP_OCCURRED_AT = "2000-01-01T00:10:00Z";
const FIXTURE_BINDING_OCCURRED_AT = "2000-01-01T00:05:00Z";

const SPLIT_GROUP_ID = "fixture-timeline-group-split";
const SPLIT_PAGE_SIZE = 50;
const SPLIT_TAIL_SIZE = 2;

function timelineMetadata(): SnapshotMetadata {
  return {
    schema_version: 1,
    snapshot_version: "fixture-timeline-snapshot-v1",
    generated_at: FIXTURE_OBSERVED_AT,
  };
}

/** PageInfo carries an opaque cursor brand; the cast stays fixture-local. */
function opaqueCursor(value: string): PageInfo["next_cursor"] {
  return value as PageInfo["next_cursor"];
}

function syncRunOrigin(): ChangeOriginDto & TimelineOriginDto {
  return { type: "sync_run", sync_run_id: "fixture-sync-run-complete" };
}

function syncRunGroupItems(): TimelineItemDto[] {
  const origin = syncRunOrigin();
  return [
    {
      change: {
        change_id: "fixture-change-sync-alpha-state",
        subject: { type: "resource", resource_id: "fixture-resource-alpha" },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [{ path: "attributes.state", before: "pending", after: "ready" }],
        origin,
      },
      version_links: [
        {
          type: "resource",
          resource_id: "fixture-resource-alpha",
          resource_version_id: "fixture-resource-version-alpha-2",
        },
      ],
    },
    {
      change: {
        change_id: "fixture-change-sync-beta-zone",
        subject: { type: "resource", resource_id: "fixture-resource-beta" },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [{ path: "attributes.zone", before: null, after: "fixture-zone-a" }],
        origin,
      },
      version_links: [],
    },
    {
      change: {
        change_id: "fixture-change-sync-alpha-limits",
        subject: { type: "resource", resource_id: "fixture-resource-alpha" },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [
          {
            path: "attributes.limits",
            before: { cores: 1 },
            after: {
              cores: 2,
              policy: { tier: "fixture-tier-standard", burst: false },
              flags: ["fixture-flag-a", "fixture-flag-b"],
            },
          },
        ],
        origin,
      },
      version_links: [
        {
          type: "resource",
          resource_id: "fixture-resource-alpha",
          resource_version_id: "fixture-resource-version-alpha-3",
        },
      ],
    },
    {
      change: {
        change_id: "fixture-change-sync-relation-latency",
        subject: {
          type: "relation",
          relation_id: "fixture-relation-provider-alpha-beta",
        },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [{ path: "attributes.latency_ms", before: 12, after: 9 }],
        origin,
      },
      version_links: [
        {
          type: "relation",
          relation_id: "fixture-relation-provider-alpha-beta",
          relation_version_id: "fixture-relation-version-alpha-beta-2",
        },
      ],
    },
    {
      change: {
        change_id: "fixture-change-sync-gamma-replicas",
        subject: { type: "resource", resource_id: "fixture-resource-gamma" },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [{ path: "attributes.replicas", before: 1, after: 3 }],
        origin,
      },
      version_links: [],
    },
  ];
}

function bindingGroup(): TimelineGroupDto {
  return {
    group_id: "fixture-timeline-group-binding",
    origin: { type: "binding", binding_id: "fixture-binding-alpha-beta" },
    occurred_at: FIXTURE_BINDING_OCCURRED_AT,
    items: [
      {
        change: {
          change_id: "fixture-change-binding-activated",
          subject: { type: "binding", binding_id: "fixture-binding-alpha-beta" },
          observed_at: FIXTURE_BINDING_OCCURRED_AT,
          fields: [{ path: "status", before: "pending", after: "active" }],
          origin: { type: "binding", binding_id: "fixture-binding-alpha-beta" },
        },
        version_links: [],
      },
    ],
  };
}

function inferenceGroup(): TimelineGroupDto {
  const origin: ChangeOriginDto & TimelineOriginDto = {
    type: "inference",
    rule_version: "fixture-rule-v1",
    input_resource_version_ids: ["fixture-resource-version-alpha-3"],
    input_relation_version_ids: ["fixture-relation-version-alpha-beta-2"],
  };
  return {
    group_id: "fixture-timeline-group-inference",
    origin,
    occurred_at: FIXTURE_OBSERVED_AT,
    items: [
      {
        change: {
          change_id: "fixture-change-inference-confidence",
          subject: {
            type: "relation",
            relation_id: "fixture-relation-inferred-alpha-beta",
          },
          observed_at: FIXTURE_OBSERVED_AT,
          fields: [{ path: "attributes.confidence", before: 0.81, after: 0.92 }],
          origin,
        },
        version_links: [
          {
            type: "relation",
            relation_id: "fixture-relation-inferred-alpha-beta",
            relation_version_id: "fixture-relation-version-inferred-1",
          },
        ],
      },
    ],
  };
}

/**
 * One complete timeline page covering every origin type, every subject type,
 * scalar and nested-object field values, a null before-image, and version
 * links. This page has no follow-up cursor.
 */
export function createTimelinePageFixture(): TimelinePageDto {
  return {
    metadata: timelineMetadata(),
    groups: [
      {
        group_id: "fixture-timeline-group-sync-run",
        origin: syncRunOrigin(),
        occurred_at: FIXTURE_GROUP_OCCURRED_AT,
        items: syncRunGroupItems(),
      },
      bindingGroup(),
      inferenceGroup(),
    ],
    page_info: { next_cursor: null },
  };
}

function splitGroupItems(startIndex: number, count: number): TimelineItemDto[] {
  return Array.from({ length: count }, (_, offset): TimelineItemDto => {
    const index = startIndex + offset;
    return {
      change: {
        change_id: `fixture-change-split-${index}`,
        subject: {
          type: "resource",
          resource_id: `fixture-resource-split-${index % 4}`,
        },
        observed_at: FIXTURE_OBSERVED_AT,
        fields: [{ path: "attributes.state", before: "pending", after: "ready" }],
        origin: { type: "sync_run", sync_run_id: "fixture-sync-run-split" },
      },
      version_links: [],
    };
  });
}

/**
 * An ordered [first, second] page pair in which the sync_run group that
 * opens the second page is the continuation of the group that closes the
 * first page (identical group_id). The first page carries 50 items of that
 * group; the second page carries the remaining tail plus one complete
 * binding group.
 */
export function createSplitGroupTimelinePagesFixture(): readonly [TimelinePageDto, TimelinePageDto] {
  const splitOrigin: TimelineOriginDto = {
    type: "sync_run",
    sync_run_id: "fixture-sync-run-split",
  };
  const first: TimelinePageDto = {
    metadata: timelineMetadata(),
    groups: [
      {
        group_id: SPLIT_GROUP_ID,
        origin: splitOrigin,
        occurred_at: FIXTURE_GROUP_OCCURRED_AT,
        items: splitGroupItems(0, SPLIT_PAGE_SIZE),
      },
    ],
    page_info: { next_cursor: opaqueCursor("fixture-cursor-2") },
  };
  const second: TimelinePageDto = {
    metadata: timelineMetadata(),
    groups: [
      {
        group_id: SPLIT_GROUP_ID,
        origin: splitOrigin,
        occurred_at: FIXTURE_GROUP_OCCURRED_AT,
        items: splitGroupItems(SPLIT_PAGE_SIZE, SPLIT_TAIL_SIZE),
      },
      {
        group_id: "fixture-timeline-group-split-tail",
        origin: { type: "binding", binding_id: "fixture-binding-split" },
        occurred_at: FIXTURE_BINDING_OCCURRED_AT,
        items: [
          {
            change: {
              change_id: "fixture-change-split-tail",
              subject: { type: "binding", binding_id: "fixture-binding-split" },
              observed_at: FIXTURE_BINDING_OCCURRED_AT,
              fields: [{ path: "status", before: "pending", after: "active" }],
              origin: { type: "binding", binding_id: "fixture-binding-split" },
            },
            version_links: [],
          },
        ],
      },
    ],
    page_info: { next_cursor: null },
  };
  return [first, second];
}
