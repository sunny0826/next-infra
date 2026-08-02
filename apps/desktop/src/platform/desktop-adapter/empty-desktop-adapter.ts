import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";

import type { DesktopAdapter } from "./desktop-adapter";

const EMPTY_CONNECTIONS: readonly ConnectionDto[] = Object.freeze([]);
const EMPTY_RELATIONS: readonly RelationDto[] = Object.freeze([]);
const EMPTY_RESOURCES: readonly ResourceDto[] = Object.freeze([]);

export class EmptyDesktopAdapter implements DesktopAdapter {
  async getSnapshotMetadata(): Promise<SnapshotMetadata | null> {
    return null;
  }

  async listResources(): Promise<readonly ResourceDto[]> {
    return EMPTY_RESOURCES;
  }

  async listRelations(): Promise<readonly RelationDto[]> {
    return EMPTY_RELATIONS;
  }

  async listConnections(): Promise<readonly ConnectionDto[]> {
    return EMPTY_CONNECTIONS;
  }
}
