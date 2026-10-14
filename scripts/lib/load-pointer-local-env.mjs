/**
 * Load gitignored `pointer.local.env` into an env object for dev/build.
 * Process / already-set keys always win; the file only fills blanks.
 */
import fs from 'node:fs';
import path from 'node:path';
import { parseEnvFile, REPO_ROOT } from './load-signing-env.mjs';

export const POINTER_LOCAL_ENV_PATH = path.join(REPO_ROOT, 'pointer.local.env');
export const POINTER_LOCAL_ENV_EXAMPLE = path.join(REPO_ROOT, 'pointer.local.env.example');

const MIRROR_PAIRS = [
  ['POINTER_EDITION', 'VITE_POINTER_EDITION'],
  ['POINTER_WEB_BASE', 'VITE_POINTER_WEB_BASE'],
  ['POINTER_DOWNLOAD_URL', 'VITE_POINTER_DOWNLOAD_URL'],
];

function isBlank(value) {
  return value == null || String(value).trim() === '';
}

function keptUnset(env) {
  return new Set(
    String(env.POINTER_LOCAL_ENV_KEEP_UNSET ?? '')
      .split(',')
      .map((key) => key.trim())
      .filter(Boolean),
  );
}

/**
 * @param {NodeJS.ProcessEnv | Record<string, string | undefined>} env
 * @param {{ logPrefix?: string, filePath?: string }} [opts]
 * @returns {{ loaded: boolean, path: string }}
 */
export function applyPointerLocalEnv(env, opts = {}) {
  const logPrefix = opts.logPrefix || '[pointer.local.env]';
  const filePath = opts.filePath || POINTER_LOCAL_ENV_PATH;
  const locked = keptUnset(env);
  let loaded = false;

  if (fs.existsSync(filePath)) {
    const parsed = parseEnvFile(fs.readFileSync(filePath, 'utf8'));
    let applied = 0;
    for (const [key, value] of Object.entries(parsed)) {
      if (locked.has(key) || !isBlank(env[key])) continue;
      env[key] = value;
      applied += 1;
    }
    loaded = true;
    console.log(
      `${logPrefix} loaded ${path.relative(REPO_ROOT, filePath)} (${applied} key(s) applied)`,
    );
  }

  for (const [from, to] of MIRROR_PAIRS) {
    if (locked.has(from) || locked.has(to)) continue;
    if (!isBlank(env[from]) && isBlank(env[to])) {
      env[to] = String(env[from]).trim();
    }
    if (!isBlank(env[to]) && isBlank(env[from])) {
      env[from] = String(env[to]).trim();
    }
  }

  if (!loaded) {
    console.log(
      `${logPrefix} no file; running standalone (no control plane). ` +
        `Copy pointer.local.env.example → pointer.local.env to bind a control plane.`,
    );
  }

  return { loaded, path: POINTER_LOCAL_ENV_PATH };
}

/** Mutate `process.env` for Vite / direct Node entrypoints. */
export function applyPointerLocalEnvToProcess(opts = {}) {
  return applyPointerLocalEnv(process.env, opts);
}
