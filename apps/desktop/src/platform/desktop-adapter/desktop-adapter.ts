import type { ConnectionDto } from "../../generated/query/ConnectionDto";
import type { RelationDto } from "../../generated/query/RelationDto";
import type { ResourceDto } from "../../generated/query/ResourceDto";
import type { SnapshotMetadata } from "../../generated/query/SnapshotMetadata";

export interface DesktopAdapter {
  getSnapshotMetadata(): Promise<SnapshotMetadata | null>;
  listResources(): Promise<readonly ResourceDto[]>;
  listRelations(): Promise<readonly RelationDto[]>;
  listConnections(): Promise<readonly ConnectionDto[]>;
}
