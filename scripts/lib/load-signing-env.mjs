import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(__dirname, '../..');
export const SIGNING_ENV_PATH = path.join(REPO_ROOT, 'signing/macos/signing.env');
export const SIGNING_ENV_EXAMPLE = path.join(REPO_ROOT, 'signing/macos/signing.env.example');

/**
 * Parse a simple KEY=VALUE env file (no export prefix, # comments).
 * @returns {Record<string, string>}
 */
export function parseEnvFile(content) {
  const env = {};
  for (const line of content.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const eq = trimmed.indexOf('=');
    if (eq <= 0) continue;
    const key = trimmed.slice(0, eq).trim();
    let value = trimmed.slice(eq + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }
  return env;
}

export function loadSigningEnv() {
  if (!fs.existsSync(SIGNING_ENV_PATH)) {
    throw new Error(
      `Missing ${path.relative(REPO_ROOT, SIGNING_ENV_PATH)}.\n` +
        `Run: npm run signing:macos:setup`,
    );
  }
  const raw = fs.readFileSync(SIGNING_ENV_PATH, 'utf8');
  return parseEnvFile(raw);
}

export function resolveRepoPath(relativeOrAbsolute) {
  if (path.isAbsolute(relativeOrAbsolute)) return relativeOrAbsolute;
  return path.resolve(REPO_ROOT, relativeOrAbsolute);
}

const PLACEHOLDER_PATTERN = /^(CHANGE_ME|PLACEHOLDER|your@email\.com|xxxx-xxxx-xxxx-xxxx)$/i;

export function isPlaceholder(value) {
  if (!value) return true;
  if (PLACEHOLDER_PATTERN.test(value)) return true;
  if (/CHANGE_ME|PLACEHOLDER/i.test(value)) return true;
  return false;
}

/**
 * @param {Record<string, string>} env
 * @param {{ notarize?: boolean }} [options]
 */
export function validateSigningEnv(env, { notarize = false } = {}) {
  const missing = [];
  const placeholders = [];

  const requireField = (key) => {
    const value = env[key]?.trim();
    if (!value) missing.push(key);
    else if (isPlaceholder(value)) placeholders.push(key);
  };

  requireField('APPLE_CERTIFICATE_PATH');
  requireField('APPLE_CERTIFICATE_PASSWORD');
  requireField('APPLE_SIGNING_IDENTITY');

  if (notarize) {
    requireField('APPLE_TEAM_ID');
  }

  const hasAppleId = env.APPLE_ID?.trim() && env.APPLE_PASSWORD?.trim();
  const hasApiKey =
    env.APPLE_API_ISSUER?.trim() &&
    env.APPLE_API_KEY?.trim() &&
    env.APPLE_API_KEY_PATH?.trim();

  if (notarize) {
    if (!hasAppleId && !hasApiKey) {
      missing.push('APPLE_ID + APPLE_PASSWORD (or APPLE_API_* trio)');
    } else if (hasAppleId) {
      if (isPlaceholder(env.APPLE_ID)) placeholders.push('APPLE_ID');
      if (isPlaceholder(env.APPLE_PASSWORD)) placeholders.push('APPLE_PASSWORD');
    }
  }

  const certPath = resolveRepoPath(env.APPLE_CERTIFICATE_PATH || '');
  if (!fs.existsSync(certPath)) {
    throw new Error(`Certificate not found: ${certPath}`);
  }

  if (missing.length) {
    throw new Error(`Missing signing.env fields: ${missing.join(', ')}`);
  }
  if (placeholders.length) {
    throw new Error(
      `Replace placeholder values in signing.env: ${placeholders.join(', ')}`,
    );
  }

  return { certPath, hasAppleId, hasApiKey };
}

/**
 * @param {Record<string, string>} fileEnv
 * @param {{ certPath: string, notarize?: boolean }} options
 */
export function buildProcessEnv(fileEnv, { certPath, notarize = false }) {
  const env = { ...process.env };
  const useKeychain =
    fileEnv.APPLE_USE_KEYCHAIN === 'true' ||
    fileEnv.APPLE_USE_KEYCHAIN === '1' ||
    fileEnv.APPLE_USE_KEYCHAIN === 'yes';

  delete env.APPLE_CERTIFICATE;
  delete env.APPLE_CERTIFICATE_PASSWORD;

  if (!useKeychain) {
    const p12 = fs.readFileSync(certPath);
    env.APPLE_CERTIFICATE = p12.toString('base64');
    env.APPLE_CERTIFICATE_PASSWORD = fileEnv.APPLE_CERTIFICATE_PASSWORD;
  }

  env.APPLE_SIGNING_IDENTITY = fileEnv.APPLE_SIGNING_IDENTITY;

  delete env.APPLE_ID;
  delete env.APPLE_PASSWORD;
  delete env.APPLE_API_ISSUER;
  delete env.APPLE_API_KEY;
  delete env.APPLE_API_KEY_PATH;

  if (fileEnv.APPLE_TEAM_ID?.trim() && !isPlaceholder(fileEnv.APPLE_TEAM_ID)) {
    env.APPLE_TEAM_ID = fileEnv.APPLE_TEAM_ID;
  } else {
    delete env.APPLE_TEAM_ID;
  }

  if (notarize) {
    if (fileEnv.APPLE_ID && fileEnv.APPLE_PASSWORD) {
      env.APPLE_ID = fileEnv.APPLE_ID;
      env.APPLE_PASSWORD = fileEnv.APPLE_PASSWORD;
    }
    if (fileEnv.APPLE_API_ISSUER) env.APPLE_API_ISSUER = fileEnv.APPLE_API_ISSUER;
    if (fileEnv.APPLE_API_KEY) env.APPLE_API_KEY = fileEnv.APPLE_API_KEY;
    if (fileEnv.APPLE_API_KEY_PATH) {
      env.APPLE_API_KEY_PATH = resolveRepoPath(fileEnv.APPLE_API_KEY_PATH);
    }
  }

  return env;
}
