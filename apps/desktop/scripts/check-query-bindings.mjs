import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const workspaceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
const bindingsPath = "apps/desktop/src/generated/query";

function bindingStatus() {
  const result = spawnSync(
    "git",
    ["status", "--porcelain", "--untracked-files=all", "--", bindingsPath],
    { cwd: workspaceRoot, encoding: "utf8" },
  );

  if (result.status !== 0) {
    process.stderr.write(result.stderr || "Failed to inspect generated query bindings.\n");
    process.exit(result.status ?? 1);
  }

  return result.stdout.trim();
}

const statusBeforeExport = bindingStatus();
const exportResult = spawnSync(
  "cargo",
  [
    "test",
    "-p",
    "next-infra-query",
    "--features",
    "typescript-bindings",
    "--test",
    "export_types",
    "--locked",
  ],
  { cwd: workspaceRoot, stdio: "inherit" },
);

if (exportResult.status !== 0) {
  process.exit(exportResult.status ?? 1);
}

const statusAfterExport = bindingStatus();
const drift = [
  ...new Set(
    [statusBeforeExport, statusAfterExport]
      .filter(Boolean)
      .flatMap((status) => status.split("\n")),
  ),
].join("\n");

if (drift) {
  process.stderr.write(
    `Query binding drift detected in ${bindingsPath}:\n${drift}\n` +
      "Run the frozen export_types test and commit every generated binding change.\n",
  );
  process.exit(1);
}

process.stdout.write("Query bindings match the Rust DTO contract.\n");
