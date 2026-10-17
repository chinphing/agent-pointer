/**
 * Updater signing key for a managed client build.
 *
 * A managed client signs updater artifacts. Standalone builds turn that off
 * and never ask for a key. The private key is created once and then reused:
 * a new key will not verify updates signed by another key.
 *
 * Missing file:
 *   --generate-updater-key creates it without asking.
 *   A terminal asks once; y generates, anything else stops.
 *   Otherwise the build stops and prints the manual command.
 * An existing file is kept. A sibling `<path>.pub` is embedded for this
 * build only. `src-tauri/tauri.conf.json` is not edited.
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import readline from 'node:readline'

export const GENERATE_UPDATER_KEY_FLAG = '--generate-updater-key'

export function expandHomePath(value, home = os.homedir()) {
  const text = String(value ?? '')
  if (text === '~') return home
  if (text.startsWith('~/') || text.startsWith('~\\')) return path.join(home, text.slice(2))
  return text
}

export function defaultUpdaterKeyPath(home = os.homedir()) {
  return path.join(home, '.tauri', 'pointer-updater.key')
}

export function updaterPubkeyPath(keyPath) {
  return `${keyPath}.pub`
}

export function splitUpdaterKeyFlag(argv) {
  const rest = []
  let generate = false
  for (const arg of argv) {
    if (arg === GENERATE_UPDATER_KEY_FLAG) generate = true
    else rest.push(arg)
  }
  return { generate, argv: rest }
}

export function updaterKeyMissingError(keyPath) {
  return [
    `企业版客户端要给更新包签名。私钥不存在：${keyPath}`,
    `Managed client builds sign updater artifacts. No private key at ${keyPath}.`,
    '生成一次，之后一直用这一份。换密钥后，已安装的客户端验不过更新。',
    'Create it once and keep using the same file. A new key will not verify updates from another key.',
    `自己生成：npm run tauri signer generate -w ${keyPath}`,
    '终端里重新执行编译命令，询问时输入 y。',
    'In a terminal, run the build again and answer y.',
    '没有交互终端时加上 --generate-updater-key，缺文件才生成，已有文件不覆盖。',
    'Without a terminal, pass --generate-updater-key. It creates the file only when missing.',
  ].join('\n')
}

export function confirmGenerate(question, input = process.stdin, output = process.stdout) {
  const rl = readline.createInterface({ input, output })
  return new Promise((resolve) => {
    rl.question(question, (answer) => {
      rl.close()
      const normalized = String(answer ?? '').trim().toLowerCase()
      resolve(normalized === 'y' || normalized === 'yes')
    })
  })
}

function isBlank(value) {
  return value == null || String(value).trim() === ''
}

function defaultSpawnGenerate(keyPath, env) {
  return spawnSync(
    'npx',
    ['tauri', 'signer', 'generate', '-w', keyPath, '--ci'],
    {
      stdio: 'inherit',
      env,
      shell: process.platform === 'win32',
    },
  )
}

/**
 * Load or create the managed updater key and return tauri args.
 * Mutates `env` with the private key material. Does not rewrite tauri.conf.json.
 *
 * @param {object} opts
 * @param {Record<string, string | undefined>} opts.env
 * @param {string[]} [opts.argv]
 * @returns {Promise<{ argv: string[], overlayPath: string | null }>}
 */
export async function prepareUpdaterSigning(opts) {
  const env = opts.env
  const home = opts.home ?? os.homedir()
  const interactive = opts.interactive ?? Boolean(process.stdin.isTTY)
  const confirm = opts.confirm ?? confirmGenerate
  const exists = opts.exists ?? fs.existsSync
  const readFile = opts.readFile ?? ((file) => fs.readFileSync(file, 'utf8'))
  const mkdir = opts.mkdir ?? ((dir) => fs.mkdirSync(dir, { recursive: true }))
  const writeFile = opts.writeFile ?? ((file, data) => fs.writeFileSync(file, data))
  const chmod = opts.chmod ?? ((file, mode) => fs.chmodSync(file, mode))
  const spawnGenerate = opts.spawnGenerate ?? ((keyPath) => defaultSpawnGenerate(keyPath, env))
  const log = opts.log ?? console.log
  const warn = opts.warn ?? console.warn
  const edition = String(env.POINTER_EDITION ?? '').trim().toLowerCase()
  const { generate, argv } = splitUpdaterKeyFlag(opts.argv ?? [])

  if (edition !== 'managed') {
    if (generate) {
      warn(
        '[tauri-build] --generate-updater-key applies to a managed client build. Standalone builds do not sign updater artifacts. Ignoring the flag.',
      )
    }
    return { argv, overlayPath: null }
  }

  const inlineKey = String(env.TAURI_SIGNING_PRIVATE_KEY ?? '').trim()
  const rawPath = String(env.TAURI_SIGNING_PRIVATE_KEY_PATH ?? '').trim()
  if (!rawPath && inlineKey) {
    if (generate) {
      warn('[tauri-build] TAURI_SIGNING_PRIVATE_KEY is already set. Keeping it and not generating a new key.')
    }
    log('[tauri-build] signing key taken from TAURI_SIGNING_PRIVATE_KEY. plugins.updater.pubkey in tauri.conf.json must match this key.')
    return { argv, overlayPath: null }
  }

  const keyPath = rawPath ? expandHomePath(rawPath, home) : defaultUpdaterKeyPath(home)
  if (rawPath && keyPath !== rawPath) {
    env.TAURI_SIGNING_PRIVATE_KEY_PATH = keyPath
    log(`[tauri-build] expanded TAURI_SIGNING_PRIVATE_KEY_PATH to ${keyPath}`)
  } else if (!rawPath) {
    env.TAURI_SIGNING_PRIVATE_KEY_PATH = keyPath
  }

  if (exists(keyPath)) {
    const material = String(readFile(keyPath) ?? '').trim()
    if (!material) throw new Error(`updater private key is empty: ${keyPath}`)
    env.TAURI_SIGNING_PRIVATE_KEY = material
    if (generate) log(`[tauri-build] updater key already exists at ${keyPath}. Keeping it.`)
    else log(`[tauri-build] signing key loaded from ${keyPath}`)
    warnIfKeyIsShared(keyPath, opts.stat ?? fs.statSync, warn)
    return embedPubkey({ keyPath, argv, exists, readFile, writeFile, log, warn })
  }

  if (inlineKey) {
    warn(`[tauri-build] updater private key path is missing (${keyPath}). Using TAURI_SIGNING_PRIVATE_KEY already set in the environment.`)
    if (generate) {
      warn('[tauri-build] not generating a new key because TAURI_SIGNING_PRIVATE_KEY is already set.')
    }
    return { argv, overlayPath: null }
  }

  let shouldGenerate = generate
  if (!shouldGenerate) {
    if (!interactive) throw new Error(updaterKeyMissingError(keyPath))
    log(`[tauri-build] 企业版客户端要给更新包签名。私钥不存在：${keyPath}`)
    log(`[tauri-build] Managed client builds sign updater artifacts. Private key not found: ${keyPath}`)
    log('[tauri-build] 同一把密钥要一直复用。The same key must be reused for later updates.')
    shouldGenerate = await confirm(
      '[tauri-build] 输入 y 生成，其他键退出。Type y to generate, anything else to stop. [y/N] ',
    )
    if (!shouldGenerate) throw new Error(updaterKeyMissingError(keyPath))
  }

  log(`[tauri-build] generating updater key at ${keyPath}`)
  mkdir(path.dirname(keyPath))
  if (!isBlank(env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
    warn(
      '[tauri-build] the generated key has no password, but TAURI_SIGNING_PRIVATE_KEY_PASSWORD is set. Unset it for this key, or generate your own key with a password.',
    )
  }
  const generated = spawnGenerate(keyPath)
  const status = generated?.status ?? 1
  if (generated?.error) throw generated.error
  if (status !== 0) throw new Error(`tauri signer generate exited ${status}`)
  const pubPath = updaterPubkeyPath(keyPath)
  if (!exists(keyPath) || !exists(pubPath)) {
    throw new Error(`tauri signer generate did not write ${keyPath} and ${pubPath}`)
  }
  try {
    chmod(keyPath, 0o600)
  } catch (err) {
    warn(`[tauri-build] could not restrict ${keyPath} to the current user: ${err instanceof Error ? err.message : err}`)
  }
  const material = String(readFile(keyPath) ?? '').trim()
  if (!material) throw new Error(`updater private key is empty after generate: ${keyPath}`)
  env.TAURI_SIGNING_PRIVATE_KEY = material
  log(`[tauri-build] updater key written to ${keyPath}. Keep this file for later updates.`)
  return embedPubkey({ keyPath, argv, exists, readFile, writeFile, log, warn })
}

function warnIfKeyIsShared(keyPath, stat, warn) {
  if (process.platform === 'win32') return
  let mode = 0
  try {
    mode = stat(keyPath).mode
  } catch (err) {
    warn(`[tauri-build] could not stat updater key ${keyPath}: ${err instanceof Error ? err.message : err}`)
    return
  }
  if ((mode & 0o077) !== 0) {
    warn(`[tauri-build] updater private key is readable by other users: ${keyPath}. Restrict it to the current user.`)
  }
}

function embedPubkey({ keyPath, argv, exists, readFile, writeFile, log, warn }) {
  const pubPath = updaterPubkeyPath(keyPath)
  if (!exists(pubPath)) {
    warn(
      `[tauri-build] no public key at ${pubPath}. This build keeps plugins.updater.pubkey from tauri.conf.json. That public key must match ${keyPath}.`,
    )
    return { argv, overlayPath: null }
  }
  const pubkey = String(readFile(pubPath) ?? '').trim()
  if (!pubkey) {
    warn(`[tauri-build] public key file is empty: ${pubPath}. This build keeps plugins.updater.pubkey from tauri.conf.json.`)
    return { argv, overlayPath: null }
  }
  const overlayPath = path.join(
    os.tmpdir(),
    `pointer-updater-pubkey-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.json`,
  )
  writeFile(overlayPath, `${JSON.stringify({ plugins: { updater: { pubkey } } })}\n`)
  log(`[tauri-build] embedding updater public key from ${pubPath} for this build. tauri.conf.json is unchanged.`)
  return { argv: [...argv, '--config', overlayPath], overlayPath }
}
