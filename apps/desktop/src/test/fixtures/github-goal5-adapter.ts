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
  "fixture-github-environment-20": {
    environment_id: 20,
    repository_id: 10,
    protected_branches: true,
    custom_branch_policies: false,
  },
  "fixture-github-deployment-30": {
    deployment_id: 30,
    repository_id: 10,
    environment: "fixture-environment",
    task: "deploy",
    production_environment: true,
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
    run_attempt: 1,
    status: "completed",
    conclusion: "success",
  },
  "fixture-github-job-60": {
    job_id: 60,
    run_id: 50,
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
