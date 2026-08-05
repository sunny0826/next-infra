import { spawn } from "node:child_process";
import { mkdtemp, rm, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "../../../..");
const desktopExecutable = path.resolve(
  process.env.NEXT_INFRA_E2E_DESKTOP ??
    path.join(
      repositoryRoot,
      "target/release/bundle/macos/Next Infra.app/Contents/MacOS/next-infra",
    ),
);
const bridgeExecutable = path.resolve(
  process.env.NEXT_INFRA_E2E_BRIDGE ??
    path.join(repositoryRoot, "target/release/next-infra-mcp"),
);
const timeoutMs = 10_000;

let desktop = null;
let bridge = null;
let temporaryHome = null;
let failure = null;

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitForExit(child, milliseconds) {
  if (child.exitCode !== null) return child.exitCode;
  return Promise.race([
    new Promise((resolve) => child.once("exit", resolve)),
    delay(milliseconds).then(() => null),
  ]);
}

async function stopChild(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGTERM");
  if ((await waitForExit(child, 2_000)) === null) {
    child.kill("SIGKILL");
    await waitForExit(child, 2_000);
  }
}

async function waitForSocket(socketPath) {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    if (desktop.exitCode !== null) {
      throw new Error(`Desktop exited before Local RPC was ready (${desktop.exitCode})`);
    }
    try {
      const metadata = await stat(socketPath);
      if ((metadata.mode & 0o777) !== 0o600) {
        throw new Error(`Local RPC socket mode is ${(metadata.mode & 0o777).toString(8)}`);
      }
      return;
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
    await delay(50);
  }
  throw new Error("Desktop Local RPC socket did not become ready");
}

function createMcpClient(child) {
  let buffer = "";
  let stderr = "";
  const pending = new Map();
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let newline;
    while ((newline = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (!line.trim()) continue;
      const message = JSON.parse(line);
      const resolve = pending.get(message.id);
      if (resolve) {
        pending.delete(message.id);
        resolve(message);
      }
    }
  });
  const request = (id, method, params) =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`MCP request ${method} timed out`));
      }, timeoutMs);
      pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  const notify = (method) => {
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method })}\n`);
  };
  return { notify, request, stderr: () => stderr };
}

async function smoke() {
  if (process.platform !== "darwin") {
    throw new Error("Desktop/Bridge smoke is macOS-only");
  }
  temporaryHome = await mkdtemp("/tmp/next-infra-mcp-e2e-");
  const environment = { ...process.env, HOME: temporaryHome };
  desktop = spawn(desktopExecutable, [], {
    env: environment,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const socketPath = path.join(
    temporaryHome,
    "Library/Application Support/Next Infra/run/next-infra-v1.sock",
  );
  await waitForSocket(socketPath);

  bridge = spawn(bridgeExecutable, [], {
    env: environment,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const client = createMcpClient(bridge);
  const initialized = await client.request(1, "initialize", {
    protocolVersion: "2026-07-28",
    capabilities: {},
    clientInfo: { name: "desktop-bridge-smoke", version: "0.1.0" },
  });
  if (!initialized.result?.capabilities?.tools) {
    throw new Error("MCP initialize did not expose tools");
  }
  client.notify("notifications/initialized");
  const tools = await client.request(2, "tools/list", {});
  if (tools.result?.tools?.length !== 7) {
    throw new Error(`expected 7 tools, received ${tools.result?.tools?.length}`);
  }
  const health = await client.request(3, "tools/call", {
    name: "get_health_summary",
    arguments: {},
  });
  if (health.result?.isError !== false) {
    throw new Error("Desktop health query returned an MCP tool error");
  }
  const observedAt = health.result?.structuredContent?.observed_at;
  if (typeof observedAt !== "string" || observedAt.length === 0) {
    throw new Error("Desktop health query omitted observed_at");
  }
  bridge.stdin.end();
  if ((await waitForExit(bridge, timeoutMs)) !== 0) {
    throw new Error(`Bridge exited unsuccessfully: ${client.stderr().trim()}`);
  }
  bridge = null;
  console.log(
    `[mcp] PASS tools=7 observed_at=${observedAt} socket_mode=600 temporary_home=${temporaryHome}`,
  );
}

try {
  await smoke();
} catch (error) {
  failure = error;
} finally {
  await stopChild(bridge);
  await stopChild(desktop);
  if (temporaryHome) {
    await rm(temporaryHome, { force: true, recursive: true });
  }
}

if (failure) {
  console.error(`[mcp] FAIL: ${failure.message}`);
  process.exitCode = 1;
}
