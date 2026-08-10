import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import { createQueryEvidenceLifecycleSnapshotFixture } from "../../test/fixtures/query-fixtures";

/**
 * Overview attention evidence adapter: serves the evidence-lifecycle snapshot
 * so getResource({ include: ["relations"] }) returns the three alpha↔beta
 * relations for beta, while tombstoned/orphaned resources return none.
 */
export class OverviewEvidenceAdapter extends MockDesktopAdapter {
  constructor() {
    super(createQueryEvidenceLifecycleSnapshotFixture());
  }
}
