import { execFile as execFileCallback, spawn } from "node:child_process";
import {
  access,
  lstat,
  mkdtemp,
  readFile,
  rm,
  stat,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFile = promisify(execFileCallback);
const pollTimeoutMs = 10_000;
const pollIntervalMs = 150;
const captureTimeoutMs = 10_000;
const renderSettleMs = 1_000;
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "../../../..");
const appBundle = path.join(
  repositoryRoot,
  "target/release/bundle/macos/Next Infra.app",
);
const requestedExecutable = process.env.NEXT_INFRA_SMOKE_EXECUTABLE;
const appExecutable = requestedExecutable
  ? path.resolve(requestedExecutable)
  : path.join(appBundle, "Contents/MacOS/next-infra");
const probeSource = path.join(scriptDirectory, "window_probe.swift");
const requestedScreenshot = process.env.NEXT_INFRA_SMOKE_SCREENSHOT;
const screenshotPath = requestedScreenshot
  ? path.resolve(requestedScreenshot)
  : path.join(
      tmpdir(),
      `next-infra-bootstrap-${process.pid}-${Date.now()}.png`,
    );

let launchedPid = null;
let helperDirectory = null;
let primaryFailure = null;

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function run(command, args, options = {}) {
  return execFile(command, args, {
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
    ...options,
  });
}

async function pathExists(candidate) {
  try {
    await lstat(candidate);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

function parseProcessTable(stdout) {
  const processes = new Map();

  for (const line of stdout.split("\n")) {
    const match = line.match(/^\s*(\d+)\s+(.+)$/);
    if (match) {
      processes.set(Number(match[1]), match[2].trim());
    }
  }

  return processes;
}

function isExactAppCommand(command) {
  return command === appExecutable || command.startsWith(`${appExecutable} `);
}

async function exactAppProcesses() {
  const { stdout } = await run("/bin/ps", ["-axo", "pid=,command="]);
  return new Map(
    [...parseProcessTable(stdout)].filter(([, command]) =>
      isExactAppCommand(command),
    ),
  );
}

async function exactCommandForPid(pid) {
  let stdout;
  try {
    ({ stdout } = await run("/bin/ps", [
      "-p",
      String(pid),
      "-o",
      "command=",
    ]));
  } catch (error) {
    if (error?.code === 1 && !error?.stdout?.trim()) {
      return null;
    }
    throw error;
  }
  const command = stdout.trim();
  return isExactAppCommand(command) ? command : null;
}

async function waitForNewPid(preexistingPids, deadline) {
  while (performance.now() < deadline) {
    const current = await exactAppProcesses();
    const newPids = [...current.keys()].filter(
      (pid) => !preexistingPids.has(pid),
    );

    if (newPids.length > 1) {
      throw new Error(
        `ambiguous launch: multiple new app PIDs appeared (${newPids.join(", ")})`,
      );
    }

    if (newPids.length === 1) {
      return newPids[0];
    }

    await delay(pollIntervalMs);
  }

  throw new Error(`no new app PID appeared within ${pollTimeoutMs}ms`);
}

async function probeWindow(probeExecutable, pid, deadline) {
  while (performance.now() < deadline) {
    if (!(await exactCommandForPid(pid))) {
      throw new Error(`launched PID ${pid} exited before showing a window`);
    }

    const { stdout } = await run(probeExecutable, [String(pid)]);
    const result = JSON.parse(stdout);
    if (result.window) {
      return result;
    }

    await delay(pollIntervalMs);
  }

  throw new Error(
    `PID ${pid} did not expose an on-screen main window within ${pollTimeoutMs}ms`,
  );
}

async function waitForHiddenWindow(probeExecutable, pid, deadline) {
  while (performance.now() < deadline) {
    if (!(await exactCommandForPid(pid))) {
      throw new Error(`PID ${pid} exited instead of hiding its window`);
    }
    const { stdout } = await run(probeExecutable, [String(pid)]);
    if (!JSON.parse(stdout).window) return;
    await delay(pollIntervalMs);
  }
  throw new Error(`PID ${pid} did not hide its main window`);
}

async function launchSecondInstanceAndWaitForExit() {
  return new Promise((resolve, reject) => {
    const child = spawn(appExecutable, [], { stdio: "ignore" });
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error("second instance did not exit after activation request"));
    }, pollTimeoutMs);
    child.once("error", reject);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function validatePng(candidate, windowBounds) {
  const [contents, fileStats] = await Promise.all([
    readFile(candidate),
    stat(candidate),
  ]);
  const expectedSignature = Buffer.from([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
  ]);

  if (!contents.subarray(0, 8).equals(expectedSignature)) {
    throw new Error("captured screenshot does not have a PNG signature");
  }
  if (contents.subarray(12, 16).toString("ascii") !== "IHDR") {
    throw new Error("captured screenshot has no PNG IHDR header");
  }
  if (fileStats.size < 4_096) {
    throw new Error(
      `captured screenshot is unexpectedly small (${fileStats.size} bytes)`,
    );
  }

  const width = contents.readUInt32BE(16);
  const height = contents.readUInt32BE(20);
  const minimumWidth = Math.floor(windowBounds.width * 0.8);
  const minimumHeight = Math.floor(windowBounds.height * 0.8);

  if (width < minimumWidth || height < minimumHeight) {
    throw new Error(
      `captured screenshot ${width}x${height} does not match window geometry ` +
        `${windowBounds.width}x${windowBounds.height}`,
    );
  }

  return { width, height, bytes: fileStats.size };
}

async function captureWindow(probeExecutable, window) {
  try {
    await run(
      probeExecutable,
      ["capture", String(window.id), screenshotPath],
      { timeout: captureTimeoutMs },
    );
    return "screencapturekit-window-id";
  } catch (error) {
    if (await pathExists(screenshotPath)) {
      throw new Error(
        `ScreenCaptureKit failed after creating ${screenshotPath}; refusing to overwrite it`,
      );
    }
    console.warn(
      `[screenshot] ScreenCaptureKit window capture unavailable: ${error.message.trim()}`,
    );
    console.warn("[screenshot] trying system screencapture");
  }

  try {
    await run(
      "/usr/sbin/screencapture",
      ["-x", "-o", `-l${window.id}`, screenshotPath],
      { timeout: captureTimeoutMs },
    );
    return "window-id";
  } catch (error) {
    if (await pathExists(screenshotPath)) {
      throw new Error(
        `system window capture failed after creating ${screenshotPath}; refusing to overwrite it`,
      );
    }
    const region = [window.x, window.y, window.width, window.height]
      .map((value) => Math.round(value))
      .join(",");
    console.warn(
      `[screenshot] window-id capture unavailable; retrying exact on-screen geometry ${region}`,
    );
    await run(
      "/usr/sbin/screencapture",
      ["-x", `-R${region}`, screenshotPath],
      { timeout: captureTimeoutMs },
    );
    return "window-geometry";
  }
}

async function terminateLaunchedPid(pid) {
  if (!(await exactCommandForPid(pid))) {
    console.log(`[cleanup] launched PID ${pid} already exited`);
    return;
  }

  process.kill(pid, "SIGTERM");
  const gracefulDeadline = performance.now() + 3_000;

  while (performance.now() < gracefulDeadline) {
    if (!(await exactCommandForPid(pid))) {
      console.log(`[cleanup] terminated launched PID ${pid}`);
      return;
    }
    await delay(100);
  }

  if (await exactCommandForPid(pid)) {
    process.kill(pid, "SIGKILL");
  }

  const forcedDeadline = performance.now() + 2_000;
  while (performance.now() < forcedDeadline) {
    if (!(await exactCommandForPid(pid))) {
      console.log(`[cleanup] force-terminated launched PID ${pid}`);
      return;
    }
    await delay(100);
  }

  throw new Error(`launched PID ${pid} remained alive after exact-PID cleanup`);
}

async function smoke() {
  if (process.platform !== "darwin") {
    throw new Error("bootstrap smoke is macOS-only");
  }

  await access(appExecutable);
  await access(probeSource);

  if (await pathExists(screenshotPath)) {
    throw new Error(
      `refusing to overwrite existing screenshot path: ${screenshotPath}`,
    );
  }
  await access(path.dirname(screenshotPath));

  helperDirectory = await mkdtemp(path.join(tmpdir(), "next-infra-bootstrap-"));
  const probeExecutable = path.join(helperDirectory, "window_probe");
  await run("/usr/bin/xcrun", [
    "swiftc",
    "-O",
    "-framework",
    "AppKit",
    "-framework",
    "ApplicationServices",
    "-framework",
    "CoreGraphics",
    "-framework",
    "ImageIO",
    "-framework",
    "ScreenCaptureKit",
    probeSource,
    "-o",
    probeExecutable,
  ]);

  const preexisting = await exactAppProcesses();
  const preexistingPids = new Set(preexisting.keys());
  console.log(
    `[launch] preexisting exact app PIDs: ${
      preexistingPids.size ? [...preexistingPids].join(", ") : "none"
    }`,
  );

  const deadline = performance.now() + pollTimeoutMs;
  if (requestedExecutable) {
    const child = spawn(appExecutable, [], { detached: true, stdio: "ignore" });
    child.unref();
    launchedPid = child.pid;
  } else {
    await run("/usr/bin/open", ["-n", appBundle]);
    launchedPid = await waitForNewPid(preexistingPids, deadline);
  }
  console.log(`[launch] selected new bundle PID: ${launchedPid}`);

  const probe = await probeWindow(probeExecutable, launchedPid, deadline);
  const window = probe.window;
  console.log(
    `[window] on-screen id=${window.id} geometry=${window.width}x${window.height}+${window.x}+${window.y}`,
  );

  if (probe.screenLocked) {
    throw new Error(
      "macOS console is locked; unlock the active user session before visual bootstrap smoke",
    );
  }

  if (!probe.screenCaptureAccess) {
    throw new Error(
      "macOS Screen Recording permission is unavailable; grant it to the invoking terminal/Codex app and rerun",
    );
  }

  await delay(renderSettleMs);
  const captureMode = await captureWindow(probeExecutable, window);
  const png = await validatePng(screenshotPath, window);

  if (!(await exactCommandForPid(launchedPid))) {
    throw new Error(`launched PID ${launchedPid} exited during screenshot capture`);
  }

  console.log(
    `[screenshot] mode=${captureMode} ${screenshotPath} (${png.width}x${png.height}, ${png.bytes} bytes)`,
  );

  if (requestedExecutable) {
    await run(probeExecutable, ["close", String(launchedPid)]);
    await waitForHiddenWindow(
      probeExecutable,
      launchedPid,
      performance.now() + pollTimeoutMs,
    );
    console.log("[lifecycle] close request hid the window and kept Runtime alive");

    await launchSecondInstanceAndWaitForExit();
    await probeWindow(
      probeExecutable,
      launchedPid,
      performance.now() + pollTimeoutMs,
    );
    console.log("[lifecycle] second instance exited and restored the first window");
  }
  console.log(
    "[visual] OCR is intentionally not asserted; inspect the retained screenshot for Next Infra, Overview, and Goal 1 placeholder",
  );
}

try {
  await smoke();
} catch (error) {
  primaryFailure = error;
} finally {
  if (launchedPid !== null) {
    try {
      await terminateLaunchedPid(launchedPid);
    } catch (error) {
      primaryFailure ??= error;
      if (primaryFailure !== error) {
        console.error(`[cleanup] ${error.message}`);
      }
    }
  }

  if (helperDirectory !== null) {
    await rm(helperDirectory, { force: true, recursive: true });
  }
}

if (primaryFailure) {
  console.error(`[smoke] FAIL: ${primaryFailure.message}`);
  process.exitCode = 1;
} else {
  console.log("[smoke] PASS");
}
