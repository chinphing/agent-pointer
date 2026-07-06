#!/usr/bin/env node
/**
 * Load signing/macos/signing.env and run Tauri build with code signing.
 * Default: Universal Binary (Intel + Apple Silicon) + sign + notarize.
 * Pass --sign-only to skip notarization. Non-signed builds use npm run build:macos.
 */
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import {
  REPO_ROOT,
  loadSigningEnv,
  validateSigningEnv,
  buildProcessEnv,
} from './lib/load-signing-env.mjs';

function run(cmd, args, env) {
  const result = spawnSync(cmd, args, {
    cwd: REPO_ROOT,
    env,
    stdio: 'inherit',
    shell: false,
  });
  if ((result.status ?? 1) !== 0) {
    process.exit(result.status ?? 1);
  }
}

function parseArgs(argv) {
  const signOnly =
    argv.includes('--sign-only') ||
    process.env.MACOS_SIGN_ONLY === '1' ||
    process.env.MACOS_SIGN_ONLY === 'true';
  const withIcons = argv.includes('--icons');
  const extraArgs = argv.filter((arg) => arg !== '--sign-only' && arg !== '--icons');
  return { signOnly, withIcons, extraArgs };
}

function resolveBuildArch(fileEnv) {
  return (
    process.env.MACOS_BUILD_ARCH ||
    fileEnv.MACOS_BUILD_ARCH ||
    'universal'
  ).toLowerCase();
}

function bundleRootForArch(arch) {
  return path.join(
    REPO_ROOT,
    'target',
    arch === 'universal' ? 'universal-apple-darwin/release/bundle' : 'release/bundle',
  );
}

function ensureUniversalRustTargets(env) {
  const result = spawnSync('rustup', ['target', 'list', '--installed'], {
    encoding: 'utf8',
    env,
  });
  const installed = result.stdout || '';
  for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
    if (installed.includes(target)) continue;
    console.log(`[macos-signed-build] Installing Rust target ${target}...`);
    run('rustup', ['target', 'add', target], env);
  }
}

function main() {
  if (process.platform !== 'darwin') {
    console.error('[macos-signed-build] Must run on macOS.');
    process.exit(1);
  }

  const { signOnly, withIcons, extraArgs } = parseArgs(process.argv.slice(2));

  try {
    const fileEnv = loadSigningEnv();
    const notarize = !signOnly;
    const validated = validateSigningEnv(fileEnv, { notarize });
    const env = buildProcessEnv(fileEnv, { ...validated, notarize });

    const arch = resolveBuildArch(fileEnv);
    const tauriArgs = ['scripts/tauri-build.mjs'];

    if (arch === 'universal') {
      ensureUniversalRustTargets(env);
      tauriArgs.push('--target', 'universal-apple-darwin');
    } else if (arch !== 'native') {
      throw new Error(`Invalid MACOS_BUILD_ARCH=${arch}; use universal or native`);
    }
    tauriArgs.push(...extraArgs);

    console.log('[macos-signed-build] Mode:', signOnly ? 'sign only (no notarization)' : 'sign + notarize');
    console.log(
      '[macos-signed-build] Certificate:',
      fileEnv.APPLE_USE_KEYCHAIN === 'true' ||
        fileEnv.APPLE_USE_KEYCHAIN === '1' ||
        fileEnv.APPLE_USE_KEYCHAIN === 'yes'
        ? 'login keychain'
        : 'import from p12',
    );
    console.log('[macos-signed-build] Signing identity:', env.APPLE_SIGNING_IDENTITY);
    if (notarize) {
      console.log('[macos-signed-build] Team ID:', env.APPLE_TEAM_ID);
      console.log('[macos-signed-build] Notarize via:', env.APPLE_ID ? 'Apple ID' : 'API key');
    }
    console.log(
      '[macos-signed-build] Target:',
      arch === 'universal'
        ? 'universal-apple-darwin (Intel + Apple Silicon)'
        : 'native (host CPU only)',
    );
    if (signOnly) {
      console.log('[macos-signed-build] Notarization skipped — users may still see Gatekeeper warnings.\n');
    } else {
      console.log('[macos-signed-build] Tauri will sign + notarize automatically.\n');
    }

    if (withIcons) {
      console.log('[macos-signed-build] Generating icons...');
      run('npm', ['run', 'icons'], env);
    }

    console.log('[macos-signed-build] Building...');
    run('node', tauriArgs, env);

    const bundleRoot = bundleRootForArch(arch);
    console.log('\n[macos-signed-build] Done. Artifacts:');
    console.log(`  ${bundleRoot}/macos/*.app`);
    console.log(`  ${bundleRoot}/dmg/*.dmg`);
    console.log('\nVerify signature:');
    console.log(`  codesign -dv --verbose=4 "${bundleRoot}/macos/"*.app`);
    if (!signOnly) {
      console.log(`  spctl -a -vv "${bundleRoot}/macos/"*.app`);
    }
  } catch (err) {
    console.error(`[macos-signed-build] ${err.message || err}`);
    process.exit(1);
  }
}

main();
