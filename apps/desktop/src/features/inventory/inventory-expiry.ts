import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";

export type InventoryStatusFilter = "all" | "attention" | "expired" | "removed";

/** 需关注: any of health!=healthy / freshness!=fresh / lifecycle!=active */
export function needsAttention(resource: ResourceDto): boolean {
  return (
    resource.health !== "healthy" ||
    resource.freshness !== "fresh" ||
    resource.lifecycle !== "active"
  );
}

/** 已失效: lifecycle is tombstoned or orphaned */
export function isRemoved(resource: ResourceDto): boolean {
  return resource.lifecycle === "tombstoned" || resource.lifecycle === "orphaned";
}

/** 已过期: freshness === "expired" */
export function isExpired(resource: ResourceDto): boolean {
  return resource.freshness === "expired";
}

export function matchesStatusFilter(
  resource: ResourceDto,
  filter: InventoryStatusFilter,
): boolean {
  switch (filter) {
    case "all":
      return true;
    case "attention":
      return needsAttention(resource);
    case "expired":
      return isExpired(resource);
    case "removed":
      return isRemoved(resource);
  }
}

export interface ConnectionExpiryRow {
  readonly connection: ConnectionDto;
  readonly expiredCount: number; // freshness expired
  readonly removedCount: number; // lifecycle tombstoned|orphaned
  readonly total: number; // expired + removed
}

/**
 * Aggregates expired/tombstoned/orphaned resources per connection over the
 * CURRENT PAGE's items. Only connections with total > 0 returned.
 * Sorted by connection display_name (localeCompare "en").
 */
export function summarizeConnectionExpiry(
  resources: readonly ResourceDto[],
  connections: readonly ConnectionDto[],
): readonly ConnectionExpiryRow[] {
  const connectionById = new Map<string, ConnectionDto>();
  for (const connection of connections) {
    connectionById.set(connection.connection_id, connection);
  }

  const expiredCountByConnection = new Map<string, number>();
  const removedCountByConnection = new Map<string, number>();
  for (const resource of resources) {
    // Skip resources whose connection is unknown to the current page.
    if (!connectionById.has(resource.connection_id)) continue;
    if (isExpired(resource)) {
      expiredCountByConnection.set(
        resource.connection_id,
        (expiredCountByConnection.get(resource.connection_id) ?? 0) + 1,
      );
    }
    if (isRemoved(resource)) {
      removedCountByConnection.set(
        resource.connection_id,
        (removedCountByConnection.get(resource.connection_id) ?? 0) + 1,
      );
    }
  }

  const rows: ConnectionExpiryRow[] = [];
  for (const connection of connections) {
    const expiredCount = expiredCountByConnection.get(connection.connection_id) ?? 0;
    const removedCount = removedCountByConnection.get(connection.connection_id) ?? 0;
    const total = expiredCount + removedCount;
    if (total > 0) {
      rows.push({ connection, expiredCount, removedCount, total });
    }
  }
  return rows.sort((left, right) =>
    left.connection.display_name.localeCompare(right.connection.display_name, "en"),
  );
}
