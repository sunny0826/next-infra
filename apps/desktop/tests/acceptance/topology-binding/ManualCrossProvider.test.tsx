import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppShell } from "../../../src/app/AppShell";
import { DesktopAdapterProvider } from "../../../src/platform/desktop-adapter/DesktopAdapterContext";
import {
  createManualRelationAdapter,
  type ManualRelationAdapter,
} from "../../../src/test/fixtures/manual-relation-adapter";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderManualShell(adapter = createManualRelationAdapter()) {
  render(
    <DesktopAdapterProvider adapter={adapter}>
      <AppShell />
    </DesktopAdapterProvider>,
  );
  return adapter;
}

async function selectGlobalResource(user: ReturnType<typeof userEvent.setup>, query: string) {
  const search = screen.getByRole("combobox", { name: "搜索本地基础设施" });
  await user.type(search, query);
  await user.click(await screen.findByRole("option", { name: new RegExp(query) }));
}

async function openTopology(
  user: ReturnType<typeof userEvent.setup>,
  sourceQuery: string,
) {
  await selectGlobalResource(user, sourceQuery);
  await user.click(screen.getByRole("button", { name: "拓扑" }));
  await screen.findByRole("heading", { level: 1, name: "拓扑" });
}

async function chooseTarget(
  user: ReturnType<typeof userEvent.setup>,
  dialog: HTMLElement,
  targetName: string,
) {
  await user.click(
    within(dialog).getByRole("button", { name: /目标资源：选择资源/ }),
  );
  await user.click(
    await within(dialog).findByRole("option", { name: new RegExp(targetName) }),
  );
}

async function createFromTopology(
  user: ReturnType<typeof userEvent.setup>,
  sourceQuery: string,
  targetName: string,
  relationOptionLabel: string,
  relationKind: string,
) {
  await openTopology(user, sourceQuery);
  await user.click(screen.getByRole("button", { name: "新增关联" }));
  const dialog = screen.getByRole("dialog", { name: "资源关系配置" });
  expect(
    within(dialog).getByRole("heading", { level: 2, name: "建立本地关系" }),
  ).toBeInTheDocument();
  await chooseTarget(user, dialog, targetName);
  await user.click(
    within(dialog).getByRole("option", { name: new RegExp(relationOptionLabel) }),
  );
  await user.click(within(dialog).getByRole("button", { name: "保存关联" }));
  await waitFor(() => {
    expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
  });
  expect((await screen.findAllByText(`${relationKind} · 人工声明`)).length).toBeGreaterThan(0);
}

describe("MREL-08 synthetic UI acceptance (no Tauri or live Provider)", () => {
  it("synthetic fixture: opens from Resource Inspector, creates GitHub writes_to, edits and disables it after re-query", async () => {
    const user = userEvent.setup();
    const adapter = renderManualShell();
    await selectGlobalResource(user, "Fixture GitHub Workflow");

    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    await waitFor(() => {
      expect(
        within(inspector).getByRole("heading", {
          level: 3,
          name: "Fixture GitHub Workflow",
        }),
      ).toBeInTheDocument();
    });
    await user.click(
      within(inspector).getByRole("button", { name: "从此资源建立关联" }),
    );
    let dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    expect(
      within(dialog).getByRole("heading", { level: 2, name: "建立本地关系" }),
    ).toBeInTheDocument();
    expect(
      within(inspector).getByRole("heading", { level: 3, name: "Fixture GitHub Workflow" }),
    ).toBeInTheDocument();

    await chooseTarget(user, dialog, "Fixture Supabase Managed Project");
    await user.click(
      within(dialog).getByRole("option", { name: /声明写入目标数据服务/ }),
    );
    await user.click(within(dialog).getByRole("button", { name: "保存关联" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "拓扑" }));
    expect(await screen.findByText("data.writes_to · 人工声明")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "已配置关系 data.writes_to" }));
    await user.click(screen.getByRole("button", { name: "编辑关联 data.writes_to" }));
    dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    expect(
      within(dialog).getByRole("heading", { level: 2, name: "编辑本地关系" }),
    ).toBeInTheDocument();
    await user.click(within(dialog).getByRole("option", { name: /依赖目标/ }));
    await user.click(within(dialog).getByRole("button", { name: "保存修改" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
    });
    expect(await screen.findByText("infra.depends_on · 人工声明")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "已配置关系 infra.depends_on" }));
    await user.click(screen.getByRole("button", { name: "编辑关联 infra.depends_on" }));
    dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    await user.click(within(dialog).getByRole("button", { name: "禁用关系" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
    });
    await expect(adapter.getBinding("fixture-binding-manual-1")).resolves.toMatchObject({
      status: "disabled",
      kind: "infra.depends_on",
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "已配置关系 infra.depends_on" })).not.toBeInTheDocument();
      expect(screen.queryByText("infra.depends_on · 人工声明")).not.toBeInTheDocument();
      expect(within(inspector).queryByRole("heading", { level: 3, name: "infra.depends_on" })).not.toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "提供方关系 automation.deploys_to" }));
    expect(within(inspector).queryByRole("button", { name: "编辑关联" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "推断关系 infra.accessed_via" }));
    expect(within(inspector).queryByRole("button", { name: "编辑关联" })).not.toBeInTheDocument();

    const serialized = document.documentElement.outerHTML;
    expect(serialized).not.toMatch(/https?:\/\/|github\.com|supabase\.co|dokploy\.com|cloudflare\.com/i);
    expect(serialized).not.toMatch(/\b(token|secret|password)\b/i);
    expect(serialized).not.toMatch(/10\.0\.|192\.168\./);
  });

  it("synthetic fixture: opens from Topology toolbar, keeps unresolved placeholders visible, and is keyboard/tab reachable on a narrow screen", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: query === "(max-width: 1180px)",
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(() => false),
      })),
    );
    const user = userEvent.setup();
    const adapter = renderManualShell();
    await openTopology(user, "Fixture Tencent CVM");
    expect(await screen.findByLabelText("未解析资源 fixture-resource-missing-host")).toBeInTheDocument();

    const create = screen.getByRole("button", { name: "新增关联" });
    create.focus();
    expect(create).toHaveFocus();
    await user.tab();
    expect(document.activeElement).not.toBe(create);
    create.focus();
    await user.keyboard("{Enter}");
    const inspector = screen.getByRole("complementary", { name: "证据检查器" });
    expect(inspector).not.toHaveAttribute("hidden");
    const dialog = screen.getByRole("dialog", { name: "资源关系配置" });
    expect(within(inspector).queryByRole("heading", { name: "建立本地关系" })).not.toBeInTheDocument();

    const targetTrigger = within(dialog).getByRole("button", { name: /目标资源：选择资源/ });
    targetTrigger.focus();
    await user.keyboard("{Enter}");
    const targetQuery = within(dialog).getByRole("searchbox", { name: "目标资源查询" });
    await user.type(targetQuery, "Fixture SSH Host");
    const targetOption = await within(dialog).findByRole("option", { name: /Fixture SSH Host/ });
    targetOption.focus();
    await user.keyboard("{Enter}");
    const kind = within(dialog).getByRole("option", { name: /通过目标入口访问/ });
    kind.focus();
    await user.keyboard("{Enter}");
    const save = within(dialog).getByRole("button", { name: "保存关联" });
    save.focus();
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "资源关系配置" })).not.toBeInTheDocument();
    });
    expect(await screen.findByText("infra.accessed_via · 人工声明")).toBeInTheDocument();
    await expect(adapter.getBinding("fixture-binding-manual-1")).resolves.toMatchObject({
      source_resource_id: "fixture-resource-tencent-cvm",
      target_resource_id: "fixture-resource-ssh-host",
      kind: "infra.accessed_via",
    });
  });

  it("synthetic fixture: creates the Supabase self-hosted deployment scenario from Topology", async () => {
    renderManualShell();
    await createFromTopology(
      userEvent.setup(),
      "Fixture Supabase Self-hosted Instance",
      "Fixture Dokploy Project",
      "通过目标控制面部署",
      "infra.deployed_via",
    );
  });

  it("synthetic fixture: creates the Cloudflare DNS routing scenario from Topology", async () => {
    renderManualShell();
    await createFromTopology(
      userEvent.setup(),
      "Fixture Cloudflare DNS",
      "Fixture Dokploy Domain",
      "路由到目标",
      "network.routes_to",
    );
  });
});
