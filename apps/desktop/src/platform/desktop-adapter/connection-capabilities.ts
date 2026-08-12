const PURGEABLE_CONNECTOR_TYPES: ReadonlySet<string> = new Set(["github", "ssh"]);

/** Local snapshot deletion is exposed only for connector types with an accepted UI flow. */
export function canPurgeConnection(connectorType: string): boolean {
  return PURGEABLE_CONNECTOR_TYPES.has(connectorType);
}
