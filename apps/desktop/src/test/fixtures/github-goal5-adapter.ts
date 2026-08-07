import type {
  GetResourceInput,
  SearchResourcesInput,
} from "../../platform/desktop-adapter/desktop-adapter";
import type { ResourceDetailDto } from "../../generated/query/ResourceDetailDto";
import { MockDesktopAdapter } from "../../platform/desktop-adapter/mock-desktop-adapter";
import {
  createGitHubConnectorCoverageFixtures,
  createGitHubGoal5SnapshotFixture,
} from "./query-fixtures";

const attributesByResourceId: Readonly<Record<string, Readonly<Record<string, string | number | boolean | null>>>> = {
  "fixture-github-repository-10": {
    repository_id: 10,
    visibility: "private",
    default_branch: "main",
    archived: false,
    disabled: false,
  },
  "fixture-github-workflow-40": {
    workflow_id: 40,
    path: ".github/workflows/fixture.yml",
    state: "active",
  },
  "fixture-github-run-50": {
    run_id: 50,
    workflow_id: 40,
    run_number: 1,
    status: "completed",
    conclusion: "success",
  },
};

export class GitHubGoal5Adapter extends MockDesktopAdapter {
  constructor() {
    super(createGitHubGoal5SnapshotFixture());
  }

  override async listConnectorCoverage() {
    return {
      metadata: (await this.searchResources()).metadata,
      items: [...createGitHubConnectorCoverageFixtures()],
    };
  }

  override async searchResources(input: SearchResourcesInput = {}) {
    const page = await super.searchResources(input);
    const query = input.query?.trim().toLocaleLowerCase("en");
    if (!query) return page;
    return {
      ...page,
      items: page.items.filter((resource) =>
        [resource.display_name, resource.kind, resource.resource_id].some((value) =>
          value.toLocaleLowerCase("en").includes(query),
        ),
      ),
    };
  }

  override async getResource(input: GetResourceInput): Promise<ResourceDetailDto> {
    const detail = await super.getResource(input);
    return {
      ...detail,
      attributes: { ...(attributesByResourceId[input.resource_id] ?? {}) },
      connector_coverage: [...createGitHubConnectorCoverageFixtures()],
    };
  }
}

export function createGitHubGoal5Adapter(): GitHubGoal5Adapter {
  return new GitHubGoal5Adapter();
}
