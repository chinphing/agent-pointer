import { describe, expect, it, vi } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {
  defaultUpdaterKeyPath,
  expandHomePath,
  prepareUpdaterSigning,
  updaterKeyMissingError,
} from './updater-signing-key.mjs'

function silent() {}

function harness(dir, env, argv, extra = {}) {
  const spawned = []
  const { home = '/home/builder', ...rest } = extra
  return {
    spawned,
    run: () => prepareUpdaterSigning({
      env,
      argv,
      home,
      interactive: false,
      confirm: () => {
        throw new Error('confirm should not run')
      },
      log: silent,
      warn: extra.warn ?? silent,
      spawnGenerate: (keyPath) => {
        spawned.push(keyPath)
        fs.mkdirSync(path.dirname(keyPath), { recursive: true })
        fs.writeFileSync(keyPath, 'PRIVATE-KEY\n')
        fs.writeFileSync(`${keyPath}.pub`, 'PUBLIC-KEY\n')
        return { status: 0 }
      },
      ...rest,
    }),
  }
}

describe('prepareUpdaterSigning', () => {
  it('expands a leading tilde and leaves other paths alone', () => {
    expect(expandHomePath('~/keys/pointer-updater.key', '/home/builder')).toBe(
      path.join('/home/builder', 'keys', 'pointer-updater.key'),
    )
    expect(expandHomePath('~', '/home/builder')).toBe('/home/builder')
    expect(expandHomePath('/abs/pointer-updater.key', '/home/builder')).toBe('/abs/pointer-updater.key')
    expect(defaultUpdaterKeyPath('/home/builder')).toBe(
      path.join('/home/builder', '.tauri', 'pointer-updater.key'),
    )
  })

  it('ignores the flag on a standalone build', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-'))
    const env = { POINTER_EDITION: '', TAURI_SIGNING_PRIVATE_KEY_PATH: path.join(dir, 'missing.key') }
    const { spawned, run } = harness(dir, env, ['--generate-updater-key', '--bundles', 'deb'])
    const result = await run()
    expect(spawned).toEqual([])
    expect(result.argv).toEqual(['--bundles', 'deb'])
    expect(result.overlayPath).toBeNull()
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBeUndefined()
  })

  it('generates a missing key when the flag is set and embeds the sibling public key', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-'))
    const keyPath = path.join(dir, 'pointer-updater.key')
    const env = { POINTER_EDITION: 'managed', TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath }
    const { spawned, run } = harness(dir, env, ['--generate-updater-key'])
    const result = await run()
    expect(spawned).toEqual([keyPath])
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBe('PRIVATE-KEY')
    expect(result.argv.at(-2)).toBe('--config')
    const overlay = JSON.parse(fs.readFileSync(result.overlayPath, 'utf8'))
    expect(overlay.plugins.updater.pubkey).toBe('PUBLIC-KEY')
    expect(fs.statSync(keyPath).mode & 0o077).toBe(0)
    fs.unlinkSync(result.overlayPath)
  })

  it('asks in a terminal and generates only after y', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-'))
    const keyPath = path.join(dir, 'pointer-updater.key')
    const env = { POINTER_EDITION: 'Managed', TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath }
    const questions = []
    const yes = harness(dir, env, [], {
      interactive: true,
      confirm: (question) => {
        questions.push(question)
        return true
      },
    })
    const result = await yes.run()
    expect(questions[0]).toMatch(/\[y\/N\]/)
    expect(yes.spawned).toEqual([keyPath])
    expect(result.overlayPath).toBeTruthy()
    fs.unlinkSync(result.overlayPath)

    const again = { POINTER_EDITION: 'managed', TAURI_SIGNING_PRIVATE_KEY_PATH: path.join(dir, 'other.key') }
    const no = harness(dir, again, [], {
      interactive: true,
      confirm: () => false,
    })
    await expect(no.run()).rejects.toThrow(/--generate-updater-key/)
    expect(no.spawned).toEqual([])
  })

  it('stops a non-interactive build that has no key and no flag', async () => {
    const keyPath = defaultUpdaterKeyPath('/home/builder')
    const env = { POINTER_EDITION: 'managed' }
    const { spawned, run } = harness('', env, [])
    await expect(run()).rejects.toThrow(updaterKeyMissingError(keyPath))
    expect(spawned).toEqual([])
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBeUndefined()
  })

  it('keeps an existing key and does not generate again', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-'))
    const keyPath = path.join(dir, 'pointer-updater.key')
    fs.writeFileSync(keyPath, 'KEEP-ME\n')
    fs.writeFileSync(`${keyPath}.pub`, 'KEEP-PUB\n')
    const env = { POINTER_EDITION: 'managed', TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath }
    const { spawned, run } = harness(dir, env, ['--generate-updater-key'])
    const result = await run()
    expect(spawned).toEqual([])
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBe('KEEP-ME')
    expect(fs.readFileSync(keyPath, 'utf8')).toBe('KEEP-ME\n')
    const overlay = JSON.parse(fs.readFileSync(result.overlayPath, 'utf8'))
    expect(overlay.plugins.updater.pubkey).toBe('KEEP-PUB')
    fs.unlinkSync(result.overlayPath)
  })

  it('warns and skips the overlay when the sibling public key is missing', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-'))
    const keyPath = path.join(dir, 'pointer-updater.key')
    fs.writeFileSync(keyPath, 'KEEP-ME\n')
    const warnings = []
    const home = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-home-'))
    const env = { POINTER_EDITION: 'managed', TAURI_SIGNING_PRIVATE_KEY_PATH: '~/pointer-updater.key' }
    const homeKey = path.join(home, 'pointer-updater.key')
    fs.copyFileSync(keyPath, homeKey)
    const { spawned, run } = harness(dir, env, [], {
      home,
      warn: (line) => warnings.push(line),
    })
    const result = await run()
    expect(spawned).toEqual([])
    expect(result.overlayPath).toBeNull()
    expect(result.argv).toEqual([])
    expect(env.TAURI_SIGNING_PRIVATE_KEY_PATH).toBe(homeKey)
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBe('KEEP-ME')
    expect(warnings.join('\n')).toMatch(/tauri\.conf\.json/)
    fs.unlinkSync(homeKey)
  })

  it('does not generate when the private key material is already in the environment', async () => {
    const env = {
      POINTER_EDITION: 'managed',
      TAURI_SIGNING_PRIVATE_KEY: 'inline-key',
    }
    const warnings = []
    const { spawned, run } = harness('', env, ['--generate-updater-key'], {
      warn: (line) => warnings.push(line),
    })
    const result = await run()
    expect(spawned).toEqual([])
    expect(result.overlayPath).toBeNull()
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBe('inline-key')
    expect(warnings.join('\n')).toMatch(/already set/)
  })

  it('creates a real keypair through tauri signer and reuses it', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-updater-real-'))
    const keyPath = path.join(dir, 'pointer-updater.key')
    const env = {
      ...process.env,
      POINTER_EDITION: 'managed',
      TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath,
    }
    delete env.TAURI_SIGNING_PRIVATE_KEY
    delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    const first = await prepareUpdaterSigning({
      env,
      argv: ['--generate-updater-key'],
      interactive: false,
      log: silent,
      warn: silent,
    })
    const privateKey = fs.readFileSync(keyPath, 'utf8')
    const pubkey = fs.readFileSync(`${keyPath}.pub`, 'utf8').trim()
    expect(privateKey.length).toBeGreaterThan(40)
    expect(env.TAURI_SIGNING_PRIVATE_KEY).toBe(privateKey.trim())
    const overlay = JSON.parse(fs.readFileSync(first.overlayPath, 'utf8'))
    expect(overlay.plugins.updater.pubkey).toBe(pubkey)
    expect(first.argv).not.toContain('--generate-updater-key')
    fs.unlinkSync(first.overlayPath)

    const spawn = vi.fn(() => {
      throw new Error('must not generate again')
    })
    const second = await prepareUpdaterSigning({
      env: { ...env, TAURI_SIGNING_PRIVATE_KEY: undefined },
      argv: ['--generate-updater-key'],
      interactive: false,
      log: silent,
      warn: silent,
      spawnGenerate: spawn,
    })
    expect(spawn).not.toHaveBeenCalled()
    expect(fs.readFileSync(keyPath, 'utf8')).toBe(privateKey)
    fs.unlinkSync(second.overlayPath)
  })
})
