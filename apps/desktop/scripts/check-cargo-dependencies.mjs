import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const workspaceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);

const metadataResult = spawnSync(
  "cargo",
  ["metadata", "--locked", "--format-version", "1"],
  {
    cwd: workspaceRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  },
);

if (metadataResult.status !== 0) {
  if (metadataResult.error) process.stderr.write(`${metadataResult.error.message}\n`);
  process.stderr.write(metadataResult.stderr);
  process.exit(metadataResult.status ?? 1);
}

const metadata = JSON.parse(metadataResult.stdout);
const workspaceIds = new Set(metadata.workspace_members);
const packages = metadata.packages.filter((item) => workspaceIds.has(item.id));
const internalNames = new Set(packages.map((item) => item.name));
const packageById = new Map(metadata.packages.map((item) => [item.id, item]));
const resolveNodeById = new Map(
  (metadata.resolve?.nodes ?? []).map((item) => [item.id, item]),
);

const allowedNormal = new Map([
  ["next-infra-core", []],
  ["next-infra-store", ["next-infra-core"]],
  ["next-infra-connector-api", ["next-infra-core"]],
  ["next-infra-normalizer", ["next-infra-connector-api", "next-infra-core"]],
  ["next-infra-connector-fixture", ["next-infra-connector-api", "next-infra-core"]],
  [
    "next-infra-connector-contract-tests",
    [
      "next-infra-connector-api",
      "next-infra-connector-catalog",
      "next-infra-connector-fixture",
      "next-infra-core",
      "next-infra-normalizer",
    ],
  ],
  ["next-infra-connector-catalog", ["next-infra-connector-api", "next-infra-core"]],
  ["next-infra-connector-github", ["next-infra-connector-api", "next-infra-core"]],
  ["next-infra-sync", ["next-infra-connector-api", "next-infra-core", "next-infra-normalizer"]],
  ["next-infra-query", ["next-infra-core"]],
  ["next-infra-runtime", ["next-infra-connector-catalog", "next-infra-core", "next-infra-query", "next-infra-store", "next-infra-sync"]],
  ["next-infra-local-rpc", ["next-infra-query"]],
  ["next-infra-host-integration", ["next-infra-local-rpc"]],
  ["next-infra-mcp", ["next-infra-local-rpc"]],
  ["next-infra-desktop-adapter", ["next-infra-core", "next-infra-host-integration", "next-infra-local-rpc", "next-infra-query", "next-infra-runtime"]],
  ["next-infra-mcp-bridge", ["next-infra-host-integration", "next-infra-local-rpc", "next-infra-mcp"]],
  [
    "next-infra-store-sync-integration",
    [
      "next-infra-connector-api",
      "next-infra-connector-fixture",
      "next-infra-core",
      "next-infra-normalizer",
      "next-infra-store",
      "next-infra-sync",
    ],
  ],
  [
    "next-infra-connector-pipeline-integration",
    [
      "next-infra-connector-api",
      "next-infra-connector-catalog",
      "next-infra-connector-contract-tests",
      "next-infra-connector-fixture",
      "next-infra-core",
      "next-infra-normalizer",
      "next-infra-store",
      "next-infra-sync",
    ],
  ],
]);

const allowedDev = new Map([
  [
    "next-infra-connector-github",
    ["next-infra-connector-contract-tests", "next-infra-normalizer"],
  ],
]);

function internalDependencies(item, kind) {
  return [
    ...new Set(
      item.dependencies
        .filter(
          (dependency) =>
            dependency.kind === kind && internalNames.has(dependency.name),
        )
        .map((dependency) => dependency.name),
    ),
  ].sort();
}

function dependencyClosure(rootId) {
  const visited = new Set();
  const pending = [rootId];

  while (pending.length > 0) {
    const packageId = pending.pop();
    if (!packageId || visited.has(packageId)) continue;

    visited.add(packageId);
    const node = resolveNodeById.get(packageId);
    for (const dependency of node?.deps ?? []) pending.push(dependency.pkg);
  }

  return visited;
}

const failures = [];

for (const item of packages) {
  const expectedNormal = [...(allowedNormal.get(item.name) ?? [])].sort();
  const actualNormal = internalDependencies(item, null);
  const expectedDev = [...(allowedDev.get(item.name) ?? [])].sort();
  const actualDev = internalDependencies(item, "dev");
  const actualBuild = internalDependencies(item, "build");

  if (JSON.stringify(actualNormal) !== JSON.stringify(expectedNormal)) {
    failures.push(`${item.name}: normal dependencies ${actualNormal} != ${expectedNormal}`);
  }

  if (JSON.stringify(actualDev) !== JSON.stringify(expectedDev)) {
    failures.push(`${item.name}: dev dependencies ${actualDev} != ${expectedDev}`);
  }

  if (actualBuild.length > 0) {
    failures.push(`${item.name}: unexpected internal build dependencies ${actualBuild}`);
  }

  const tauriClosure = [...dependencyClosure(item.id)]
    .map((packageId) => packageById.get(packageId)?.name)
    .filter(
      (name) => name === "tauri" || name?.startsWith("tauri-"),
    )
    .sort();

  if (item.name !== "next-infra-desktop-adapter" && tauriClosure.length > 0) {
    failures.push(`${item.name}: forbidden Tauri closure ${tauriClosure}`);
  }

  if (item.name === "next-infra-desktop-adapter" && !tauriClosure.includes("tauri")) {
    failures.push("next-infra-desktop-adapter: Tauri missing from dependency closure");
  }
}

if (packages.length !== 18) {
  failures.push(`workspace package count ${packages.length} != 18`);
}

if (failures.length > 0) {
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exit(1);
}

process.stdout.write("Cargo dependency boundaries are valid.\n");
