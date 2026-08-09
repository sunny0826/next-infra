import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RelationDto } from "../../../generated/query/RelationDto";
import type { ResourceDto } from "../../../generated/query/ResourceDto";
import { DesktopAdapterProvider } from "../../../platform/desktop-adapter/DesktopAdapterContext";
import {
  createManualRelationAdapter,
  createManualRelationSnapshotFixture,
  type ManualRelationAdapter,
} from "../../../test/fixtures/manual-relation-adapter";

import { RelationBuilder } from "./RelationBuilder";
import { MANUAL_RELATION_KIND_OPTIONS } from "./relation-vocabulary";

afterEach(cleanup);

const snapshot = createManualRelationSnapshotFixture();

function fixtureResource(resourceId: string): ResourceDto {
  const resource = snapshot.resources.find((item) => item.resource_id === resourceId);
  if (resource === undefined) throw new Error(`Missing fixture resource ${resourceId}`);
  return resource;
}

function fixtureRelation(relationId: string): RelationDto {
  const relation = snapshot.relations.find((item) => item.relation_id === relationId);
  if (relation === undefined) throw new Error(`Missing fixture relation ${relationId}`);
  return relation;
}

function renderBuilder(
  adapter: ManualRelationAdapter = createManualRelationAdapter(),
  props: {
    source?: ResourceDto | null;
    relation?: RelationDto | null;
  } = {},
) {
  const onSaved = vi.fn();
  const onCancel = vi.fn();
  render(
    <DesktopAdapterProvider adapter={adapter}>
      <RelationBuilder {...props} onCancel={onCancel} onSaved={onSaved} />
    </DesktopAdapterProvider>,
  );
  return { adapter, onSaved, onCancel };
}

describe("RelationBuilder", () => {
  it("shows the frozen vocabulary, configured evidence notice, and direction preview", () => {
    const source = fixtureResource("fixture-resource-supabase-self-hosted-instance");
    renderBuilder(undefined, { source });

    expect(screen.getByRole("note")).toHaveTextContent(
      "这是你手工声明的本地关系，未通过 Provider 验证，不会执行外部操作。",
    );
    expect(screen.getByText("Fixture Supabase Self-hosted Instance")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /infra\.deployed_via/ })).toBeInTheDocument();
    for (const option of MANUAL_RELATION_KIND_OPTIONS) {
      expect(screen.getByRole("option", { name: new RegExp(option.id) })).toBeInTheDocument();
    }
    expect(screen.getByText(/Fixture Supabase Self-hosted Instance → 通过目标控制面部署/)).toBeInTheDocument();
  });

  it("uses bounded global resource search with connector and kind filters", async () => {
    const user = userEvent.setup();
    const adapter = createManualRelationAdapter();
    const search = vi.spyOn(adapter, "searchResources");
    renderBuilder(adapter, {
      source: fixtureResource("fixture-resource-github-workflow"),
    });

    await user.click(screen.getByRole("button", { name: /目标资源：选择资源/ }));
    const query = screen.getByRole("searchbox", { name: "目标资源查询" });
    await user.type(query, "managed");
    await user.click(await screen.findByRole("button", { name: "supabase-managed" }));
    await user.type(screen.getByRole("searchbox", { name: "目标资源类型筛选" }), "supabase.managed.project");
    await user.click(await screen.findByRole("option", { name: /Fixture Supabase Managed Project/ }));

    await waitFor(() => {
      expect(search).toHaveBeenCalledWith(expect.objectContaining({
        limit: 20,
        query: expect.any(String),
        connector_types: ["supabase-managed"],
        kinds: ["supabase.managed.project"],
      }));
    });
    expect(screen.getByRole("button", { name: /目标资源：Fixture Supabase Managed Project/ })).toBeInTheDocument();
  });

  it("creates a configured cross-provider relation and reports the authoritative callback", async () => {
    const user = userEvent.setup();
    const adapter = createManualRelationAdapter();
    const { onSaved } = renderBuilder(adapter, {
      source: fixtureResource("fixture-resource-github-workflow"),
    });

    await user.click(screen.getByRole("button", { name: /目标资源：选择资源/ }));
    await user.click(await screen.findByRole("option", { name: /Fixture Supabase Managed Project/ }));
    await user.click(screen.getByRole("option", { name: /声明写入目标数据服务/ }));
    await user.click(screen.getByRole("button", { name: "保存关联" }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(onSaved).toHaveBeenCalledWith({
      action: "created",
      sourceResourceId: "fixture-resource-github-workflow",
      targetResourceId: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
    });
    const topology = await adapter.getTopology({
      focus_resource_id: "fixture-resource-github-workflow",
    });
    expect(topology.edges).toContainEqual(expect.objectContaining({
      source_resource_id: "fixture-resource-github-workflow",
      target_resource_id: "fixture-resource-supabase-managed-project",
      kind: "data.writes_to",
      evidence_type: "configured",
    }));
  });

  it("prefills configured edits, updates the binding, and disables it", async () => {
    const user = userEvent.setup();
    const adapter = createManualRelationAdapter();
    const relation = fixtureRelation("fixture-relation-fixture-binding-supabase-dokploy");
    const source = fixtureResource("fixture-resource-supabase-self-hosted-instance");
    const { onSaved } = renderBuilder(adapter, { relation, source });

    expect(screen.getByRole("button", { name: /来源资源：Fixture Supabase Self-hosted Instance/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /目标资源：fixture-resource-dokploy-application/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存修改" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: /目标资源：fixture-resource-dokploy-application/ }));
    await user.click(await screen.findByRole("option", { name: /Fixture Dokploy Project/ }));
    await user.click(screen.getByRole("option", { name: /依赖目标/ }));
    await user.click(screen.getByRole("button", { name: "保存修改" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));

    const updated = await adapter.getBinding("fixture-binding-supabase-dokploy");
    expect(updated).toMatchObject({
      target_resource_id: "fixture-resource-dokploy-project",
      kind: "infra.depends_on",
      status: "active",
    });

    await user.click(screen.getByRole("button", { name: "禁用关系" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(2));
    await expect(adapter.getBinding("fixture-binding-supabase-dokploy")).resolves.toMatchObject({ status: "disabled" });
  });

  it("exposes empty and duplicate errors and keeps non-configured relations disabled", async () => {
    const user = userEvent.setup();
    const adapter = createManualRelationAdapter();
    const { onSaved } = renderBuilder(adapter, {
      source: fixtureResource("fixture-resource-supabase-self-hosted-instance"),
    });

    await user.click(screen.getByRole("button", { name: /目标资源：选择资源/ }));
    await user.type(screen.getByRole("searchbox", { name: "目标资源查询" }), "fixture-resource-does-not-exist");
    expect(await screen.findByText("没有匹配的本地资源。")).toBeInTheDocument();
    const save = screen.getByRole("button", { name: "保存关联" });
    expect(save).toBeEnabled();
    await user.click(save);
    expect(await screen.findByRole("alert")).toHaveTextContent("请选择来源、关系类型和目标资源");

    await user.clear(screen.getByRole("searchbox", { name: "目标资源查询" }));
    await user.click(await screen.findByRole("option", { name: /Fixture Dokploy Application/ }));
    await user.click(screen.getByRole("button", { name: "保存关联" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith({
      action: "existing",
      sourceResourceId: "fixture-resource-supabase-self-hosted-instance",
      targetResourceId: "fixture-resource-dokploy-application",
      kind: "infra.deployed_via",
    }));

    cleanup();
    const provider = fixtureRelation("fixture-relation-github-dokploy");
    renderBuilder(adapter, { relation: provider });
    expect(screen.getByText(/只有 configured 关系可以编辑/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存关联" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "禁用关系" })).not.toBeInTheDocument();
  });

  it("keeps temporal conflicts visible instead of reporting an existing relation", async () => {
    const user = userEvent.setup();
    const adapter = createManualRelationAdapter();
    vi.spyOn(adapter, "createBinding").mockRejectedValue(
      Object.assign(new Error("fixture temporal conflict"), {
        code: "binding_temporal_conflict",
      }),
    );
    const { onSaved } = renderBuilder(adapter, {
      source: fixtureResource("fixture-resource-supabase-self-hosted-instance"),
    });

    await user.click(screen.getByRole("button", { name: /目标资源：选择资源/ }));
    await user.click(await screen.findByRole("option", { name: /Fixture Dokploy Project/ }));
    await user.click(screen.getByRole("button", { name: "保存关联" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "关系刚刚发生变化，请刷新拓扑后重试。",
    );
    expect(onSaved).not.toHaveBeenCalled();
  });
});
