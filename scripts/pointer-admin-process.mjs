#!/usr/bin/env node
/**
 * Cross-platform pointer-admin (control plane) process manager
 * (start / stop / restart / status). Sibling of `pointer-server-process.mjs`.
 *
 *   npm run admin:start     # start in the background, write a PID file
 *   npm run admin:stop
 *   npm run admin:restart
 *   npm run admin:status    # exit 0 when running, 1 when stopped
 *
 * Two differences from the server sibling, both about the control plane being
 * a two-half product:
 *
 * 1. `POINTER_ADMIN_STATIC_DIR` is resolved here when the caller did not set
 *    it — an admin process that starts without static assets serves a blank
 *    page, which is a miserable thing to debug. Preference order:
 *      a. whatever the caller exported (unchanged)
 *      b. the assembled bundle's `web/` (npm run admin:build)
 *      c. `admin/web/dist` from a local dev build
 *    If none exists we refuse to start and say how to fix it.
 *
 * 2. `POINTER_ADMIN_DATA_DIR` and `POINTER_ADMIN_DB_URL` are left alone: they
 *    point at real state, and guessing a location for someone's database is
 *    not this script's job.
 */
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { platformTag } from "./lib/platform-tag.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const BIN_NAME =
  process.platform === "win32" ? "pointer-admin.exe" : "pointer-admin";
const BIN_DIR = path.join(ROOT, "target", "release");
const BIN_PATH = path.join(BIN_DIR, BIN_NAME);
const PID_FILE = path.join(BIN_DIR, ".pointer-admin.pid");
const LOG_FILE = path.join(BIN_DIR, "pointer-admin.log");
const BUNDLE_WEB = path.join(
  BIN_DIR,
  "pointer-admin-bundle",
  `pointer-admin-${platformTag()}`,
  "web",
);
const DEV_WEB = path.join(ROOT, "admin", "web", "dist");

const command = process.argv[2] ?? "status";

const log = (msg) => console.log(`[pointer-admin] ${msg}`);

function usage() {
  console.error(
    "Usage: node scripts/pointer-admin-process.mjs <start|stop|restart|status>",
  );
  process.exit(1);
}

function ensureBinary() {
  if (fs.existsSync(BIN_PATH)) return;
  console.error(`[pointer-admin] 找不到二进制：${BIN_PATH}`);
  console.error("[pointer-admin] 先运行 npm run admin:build。");
  process.exit(1);
}

/**
 * Resolve the static directory the binary will serve. Returns the value to
 * export, or exits with a fixable message when neither half is present.
 */
function resolveStaticDir() {
  const explicit = (process.env.POINTER_ADMIN_STATIC_DIR || "").trim();
  if (explicit) return explicit;
  if (fs.existsSync(path.join(BUNDLE_WEB, "index.html"))) return BUNDLE_WEB;
  if (fs.existsSync(path.join(DEV_WEB, "index.html"))) return DEV_WEB;
  console.error("[pointer-admin] 找不到前端产物，启动后会是白屏。");
  console.error(`[pointer-admin] 试过：${BUNDLE_WEB}`);
  console.error(`[pointer-admin] 试过：${DEV_WEB}`);
  console.error(
    "[pointer-admin] 先运行 npm run admin:build（或显式设置 POINTER_ADMIN_STATIC_DIR）。",
  );
  process.exit(1);
}

function readPid() {
  if (!fs.existsSync(PID_FILE)) return null;
  const raw = fs.readFileSync(PID_FILE, "utf8").trim();
  const pid = Number.parseInt(raw, 10);
  return Number.isFinite(pid) && pid > 0 ? pid : null;
}

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function removePidFile() {
  if (fs.existsSync(PID_FILE)) fs.unlinkSync(PID_FILE);
}

function stopProcess() {
  const pid = readPid();
  if (!pid) {
    log("未在运行（没有 PID 文件）。");
    return 0;
  }
  if (!isAlive(pid)) {
    log(`PID 文件已过期（${pid}），清理。`);
    removePidFile();
    return 0;
  }

  if (process.platform === "win32") {
    const result = spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
      stdio: "inherit",
    });
    if (result.status !== 0) {
      console.error(`[pointer-admin] 停止 PID ${pid} 失败。`);
      return result.status ?? 1;
    }
  } else {
    process.kill(pid, "SIGTERM");
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      if (!isAlive(pid)) break;
      spawnSync("sleep", ["0.2"]);
    }
    if (isAlive(pid)) process.kill(pid, "SIGKILL");
  }

  removePidFile();
  log(`已停止（PID ${pid}）。`);
  return 0;
}

function startProcess() {
  ensureBinary();
  const staticDir = resolveStaticDir();

  const existing = readPid();
  if (existing && isAlive(existing)) {
    log(`已在运行（PID ${existing}）。`);
    return 0;
  }
  if (existing) removePidFile();

  fs.mkdirSync(BIN_DIR, { recursive: true });
  const logFd = fs.openSync(LOG_FILE, "a");

  const child = spawn(BIN_PATH, [], {
    cwd: BIN_DIR,
    detached: true,
    stdio: ["ignore", logFd, logFd],
    windowsHide: true,
    env: { ...process.env, POINTER_ADMIN_STATIC_DIR: staticDir },
  });
  fs.closeSync(logFd);

  child.unref();
  fs.writeFileSync(PID_FILE, String(child.pid), "utf8");
  log(`已启动（PID ${child.pid}）。`);
  log(`静态资源：${staticDir}`);
  log(`日志：${LOG_FILE}`);
  return 0;
}

function statusProcess() {
  const pid = readPid();
  if (!pid) {
    log("状态：已停止");
    return 1;
  }
  if (!isAlive(pid)) {
    log(`状态：已停止（PID ${pid} 已过期）`);
    removePidFile();
    return 1;
  }
  log(`状态：运行中（PID ${pid}）`);
  log(`日志：${LOG_FILE}`);
  return 0;
}

switch (command) {
  case "start":
    process.exit(startProcess());
    break;
  case "stop":
    process.exit(stopProcess());
    break;
  case "restart": {
    stopProcess();
    process.exit(startProcess());
    break;
  }
  case "status":
    process.exit(statusProcess());
    break;
  default:
    usage();
}
