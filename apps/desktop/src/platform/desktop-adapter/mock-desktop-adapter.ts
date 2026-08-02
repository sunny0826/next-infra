import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";

import type { DesktopAdapter } from "./desktop-adapter";

export interface DesktopAdapterSnapshot {
  readonly metadata: SnapshotMetadata | null;
  readonly resources: readonly ResourceDto[];
  readonly relations: readonly RelationDto[];
  readonly connections: readonly ConnectionDto[];
}

function copyMetadata(metadata: SnapshotMetadata | null): SnapshotMetadata | null {
  return metadata === null ? null : { ...metadata };
}

function copyItems<T extends object>(items: readonly T[]): T[] {
  return items.map((item) => ({ ...item }));
}

function copySnapshot(snapshot: DesktopAdapterSnapshot): DesktopAdapterSnapshot {
  return {
    metadata: copyMetadata(snapshot.metadata),
    resources: copyItems(snapshot.resources),
    relations: copyItems(snapshot.relations),
    connections: copyItems(snapshot.connections),
  };
}

export class MockDesktopAdapter implements DesktopAdapter {
  readonly #snapshot: DesktopAdapterSnapshot;

  constructor(snapshot: DesktopAdapterSnapshot) {
    this.#snapshot = copySnapshot(snapshot);
  }

  async getSnapshotMetadata(): Promise<SnapshotMetadata | null> {
    return copyMetadata(this.#snapshot.metadata);
  }

  async listResources(): Promise<readonly ResourceDto[]> {
    return copyItems(this.#snapshot.resources);
  }

  async listRelations(): Promise<readonly RelationDto[]> {
    return copyItems(this.#snapshot.relations);
  }

  async listConnections(): Promise<readonly ConnectionDto[]> {
    return copyItems(this.#snapshot.connections);
  }
}
