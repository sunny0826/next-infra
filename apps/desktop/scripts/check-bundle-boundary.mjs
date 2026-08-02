import { existsSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const workspaceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
const bundleRoot = path.join(workspaceRoot, "target", "release", "bundle");

function findAppBundles(directory) {
  if (!existsSync(directory)) return [];

  const bundles = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (!entry.isDirectory()) continue;
    if (entry.name.endsWith(".app")) bundles.push(entryPath);
    else bundles.push(...findAppBundles(entryPath));
  }
  return bundles;
}

function containsBridge(directory) {
  if (!existsSync(directory)) return false;

  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === "next-infra-mcp") return true;
    if (entry.isDirectory() && containsBridge(path.join(directory, entry.name))) return true;
  }
  return false;
}

const appBundles = findAppBundles(bundleRoot);

if (appBundles.length !== 1) {
  throw new Error(`expected one .app bundle under ${bundleRoot}, found ${appBundles.length}`);
}

const [appBundle] = appBundles;
const macosDirectory = path.join(appBundle, "Contents", "MacOS");
const mainBinary = path.join(macosDirectory, "next-infra");
const executables = readdirSync(macosDirectory).sort();

if (!existsSync(mainBinary) || !statSync(mainBinary).isFile()) {
  throw new Error(`missing Desktop main binary: ${mainBinary}`);
}

if (executables.length !== 1 || executables[0] !== "next-infra") {
  throw new Error(`unexpected Desktop executables: ${executables.join(", ")}`);
}

if (containsBridge(appBundle)) {
  throw new Error(`next-infra-mcp must not be present in ${appBundle}`);
}

process.stdout.write(`Bundle boundary is valid: ${appBundle}\n`);
