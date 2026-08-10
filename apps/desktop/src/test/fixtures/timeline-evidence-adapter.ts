import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";

import { createQueryEvidenceLifecycleSnapshotFixture } from "./query-fixtures";

/**
 * Serves the evidence-lifecycle snapshot so timeline items can resolve
 * resource subjects to endpoint resources and incident relations. The base
 * MockDesktopAdapter.getResource already filters relations by endpoint, so
 * this subclass only fixes the snapshot.
 */
export class TimelineEvidenceAdapter extends MockDesktopAdapter {
  constructor() {
    super(createQueryEvidenceLifecycleSnapshotFixture());
  }
}
