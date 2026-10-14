import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { applyPointerLocalEnv } from './lib/load-pointer-local-env.mjs'
import {
  buildPlan,
  defaultReleaseBinary,
  devTauriArgs,
  devViteArgs,
  findOnPath,
  localAdminCredentials,
  missingLaunchArtifact,
  modeKind,
  parseArgs,
  releaseBuildInvocation,
  releaseBuildScript,
  REPO_ROOT,
  runReleaseBuild,
  selectBinClient,
  STANDALONE_BUILD_KEEP_UNSET,
} from './quick-deploy.mjs'

const domains = {
  apiBase: 'https://pointer-api.example.com',
  webBase: 'https://pointer.example.com',
  annotateBase: 'https://pointer-som.example.com',
}

function plan(mode, extra = {}) {
  return buildPlan({
    mode,
    runtime: 'dev',
    stateDir: path.join(os.tmpdir(), 'pointer-quick-deploy-test', mode),
    domains,
    credentials: localAdminCredentials(() => Buffer.from('0123456789abcdef0123456789abcdef')),
    baseEnv: {
      POINTER_EDITION: 'managed',
      POINTER_API_BASE: 'https://leak.example',
      PATH: process.env.PATH,
    },
    ...extra,
  })
}

describe('quick-deploy plans', () => {
  it('standalone-server isolates admin auth and drops inherited control-plane env', () => {
    const result = plan('standalone-server')
    expect(modeKind('standalone-server').authMode).toBe('standalone')
    expect(result.processes.map((proc) => proc.name)).toEqual(['server', 'web'])
    const env = result.processes[0].env
    expect(env.POINTER_DEPLOYMENT_MODE).toBe('standalone')
    expect(env.POINTER_EDITION).toBeUndefined()
    expect(env.POINTER_API_BASE).toBeUndefined()
    expect(env.POINTER_SERVER_ADMIN_USERNAME).toBe('admin')
    expect(env.POINTER_SERVER_ADMIN_PASSWORD_HMAC).toMatch(/^[0-9a-f]{64}$/)
    expect(env.POINTER_SERVER_ADDR).toBe('127.0.0.1:8787')
    expect(env.POINTER_SERVER_STATIC_DIR).toBeUndefined()
    expect(result.runtime).toBe('dev')
    expect(result.webUrl).toBe('http://127.0.0.1:1420/')
    expect(result.usesVite).toBe(true)
    expect(result.processes[1].command).toBe('node')
    expect(result.processes[1].args).toEqual(devViteArgs(REPO_ROOT))
    expect(result.processes.some((proc) => proc.command === 'npm' || proc.command === 'npx')).toBe(false)
    expect(missingLaunchArtifact(result)).toBeNull()
  })

  it('standalone-client starts tauri dev without a deployment mode', () => {
    const result = plan('standalone-client')
    expect(result.authMode).toBeNull()
    expect(result.processes).toHaveLength(1)
    expect(result.processes[0].command).toBe('node')
    expect(result.processes[0].args).toEqual(devTauriArgs(REPO_ROOT))
    expect(result.processes[0].env.POINTER_EDITION).toBeUndefined()
    expect(result.processes[0].env.POINTER_DEPLOYMENT_MODE).toBeUndefined()
    expect(result.processes[0].env.POINTER_APP_DATA_DIR).toContain('standalone-client')
  })

  it('managed-server requires domains and records platform mode', () => {
    expect(() => plan('managed-server', { domains: {} })).toThrow(/control-plane domains/)
    const result = plan('managed-server')
    const env = result.processes[0].env
    expect(result.authMode).toBe('platform')
    expect(env.POINTER_EDITION).toBe('managed')
    expect(env.VITE_POINTER_EDITION).toBe('managed')
    expect(env.POINTER_DEPLOYMENT_MODE).toBe('platform')
    expect(env.POINTER_API_BASE).toBe(domains.apiBase)
    expect(env.POINTER_WEB_BASE).toBe(domains.webBase)
    expect(env.VITE_POINTER_WEB_BASE).toBe(domains.webBase)
    expect(env.COMPUTER_ANNOTATE_API_BASE).toBe(domains.annotateBase)
    expect(env.POINTER_SERVER_ADMIN_USERNAME).toBeUndefined()
  })

  it('managed-client passes the same domains to tauri dev', () => {
    const result = plan('managed-client')
    expect(result.processes[0].command).toBe('node')
    expect(result.processes[0].args).toEqual(devTauriArgs(REPO_ROOT))
    expect(result.processes[0].env.POINTER_EDITION).toBe('managed')
    expect(result.processes[0].env.POINTER_API_BASE).toBe(domains.apiBase)
    expect(result.webUrl).toBeNull()
  })

  it('parses domain flags', () => {
    const parsed = parseArgs([
      'start',
      'managed-server',
      '--api-base',
      'https://api.example',
      '--web-base',
      'https://web.example',
      '--annotate-base',
      'https://som.example',
    ])
    expect(parsed.command).toBe('start')
    expect(parsed.mode).toBe('managed-server')
    expect(parsed.domains.annotateBase).toBe('https://som.example')
    expect(parsed.runtime).toBe('bin')
    expect(parsed.build).toBe(false)
  })

  it('defaults an omitted runtime to the release binary', () => {
    const result = buildPlan({
      mode: 'standalone-client',
      stateDir: path.join(os.tmpdir(), 'pointer-quick-deploy-test', 'default-bin'),
      domains,
    })
    expect(result.runtime).toBe('bin')
    expect(result.processes).toHaveLength(1)
    expect(result.processes[0].command).toBe(defaultReleaseBinary(REPO_ROOT, 'client'))
    expect(result.processes[0].args).toEqual([])
  })

  it('parses --runtime and rejects an unknown runtime', () => {
    const parsed = parseArgs(['start', 'standalone-client', '--runtime', 'bin'])
    expect(parsed.runtime).toBe('bin')
    expect(parsed.mode).toBe('standalone-client')
    expect(parsed.build).toBe(false)
    expect(() => parseArgs(['plan', 'standalone-server', '--runtime', 'package'])).toThrow(/unknown runtime/)
  })

  it('accepts a valueless --build and refuses it on the dev runtime', () => {
    const parsed = parseArgs(['start', 'standalone-client', '--build', '--runtime', 'bin'])
    expect(parsed.build).toBe(true)
    expect(parsed.runtime).toBe('bin')
    expect(parsed.mode).toBe('standalone-client')
    expect(() => parseArgs(['start', 'standalone-client', '--runtime', 'dev', '--build'])).toThrow(/--build/)
  })

  it('bin server runs the release binary and serves dist on the API port', () => {
    const result = plan('standalone-server', {
      runtime: 'bin',
      baseEnv: {
        POINTER_EDITION: 'managed',
        POINTER_SERVER_STATIC_DIR: '/tmp/leak',
        PATH: process.env.PATH,
      },
    })
    expect(result.processes.map((proc) => proc.name)).toEqual(['server'])
    expect(result.processes[0].command).toBe(defaultReleaseBinary(REPO_ROOT, 'server'))
    expect(result.processes[0].args).toEqual([])
    expect(result.processes[0].env.POINTER_SERVER_STATIC_DIR).toBe(path.join(REPO_ROOT, 'dist'))
    expect(result.processes[0].env.POINTER_EDITION).toBeUndefined()
    expect(result.processes[0].env.POINTER_SERVER_ADMIN_USERNAME).toBe('admin')
    expect(result.webUrl).toBe('http://127.0.0.1:8787/')
    expect(result.usesVite).toBe(false)
    expect(result.stateDir).toContain(`${path.sep}standalone-server`)
  })

  it('bin managed-server keeps platform mode and does not create a local admin', () => {
    const result = plan('managed-server', { runtime: 'bin' })
    const env = result.processes[0].env
    expect(result.authMode).toBe('platform')
    expect(env.POINTER_EDITION).toBe('managed')
    expect(env.POINTER_DEPLOYMENT_MODE).toBe('platform')
    expect(env.POINTER_API_BASE).toBe(domains.apiBase)
    expect(env.POINTER_SERVER_ADMIN_USERNAME).toBeUndefined()
    expect(env.POINTER_SERVER_STATIC_DIR).toBe(path.join(REPO_ROOT, 'dist'))
    expect(result.webUrl).toBe('http://127.0.0.1:8787/')
  })

  it('bin client launches pointer-app with no tauri args', () => {
    const standalone = plan('standalone-client', { runtime: 'bin' })
    expect(standalone.processes).toHaveLength(1)
    expect(standalone.processes[0].command).toBe(defaultReleaseBinary(REPO_ROOT, 'client'))
    expect(standalone.processes[0].args).toEqual([])
    expect(standalone.processes[0].env.POINTER_EDITION).toBeUndefined()
    expect(standalone.webUrl).toBeNull()

    const managed = plan('managed-client', { runtime: 'bin' })
    expect(managed.processes[0].command).toBe(defaultReleaseBinary(REPO_ROOT, 'client'))
    expect(managed.processes[0].env.POINTER_EDITION).toBe('managed')
    expect(managed.processes[0].env.POINTER_API_BASE).toBe(domains.apiBase)
    expect(managed.processes[0].env.COMPUTER_ANNOTATE_API_BASE).toBe(domains.annotateBase)
  })

  it('refuses a bin launch when the release binary or web bundle is missing', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-quick-deploy-'))
    const serverBin = path.join(root, 'pointer-server')
    const missingServer = plan('standalone-server', {
      runtime: 'bin',
      root,
      binaries: { server: serverBin },
    })
    expect(missingLaunchArtifact(missingServer)).toMatch(/node scripts\/build-server\.mjs/)

    fs.writeFileSync(serverBin, '')
    const missingDist = plan('standalone-server', {
      runtime: 'bin',
      root,
      binaries: { server: serverBin },
    })
    expect(missingLaunchArtifact(missingDist)).toMatch(/node scripts\/build-server\.mjs which writes it/)

    fs.mkdirSync(path.join(root, 'dist'))
    expect(missingLaunchArtifact(missingDist)).toBeNull()

    const missingClient = plan('standalone-client', { runtime: 'bin', root })
    expect(missingLaunchArtifact(missingClient)).toMatch(/--build/)
  })

  it('uses a local release binary, then an installed pointer-app, and does not download', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-quick-deploy-client-'))
    const installed = path.join(root, 'bin', 'pointer-app')
    fs.mkdirSync(path.dirname(installed), { recursive: true })
    fs.writeFileSync(installed, '')
    expect(findOnPath('pointer-app', path.dirname(installed))).toBe(installed)
    expect(selectBinClient(root, { pathValue: path.dirname(installed) })).toEqual({
      command: installed,
      args: [],
      source: 'installed',
    })
    expect(selectBinClient(root, { pathValue: '' })).toBeNull()

    const local = defaultReleaseBinary(root, 'client')
    fs.mkdirSync(path.dirname(local), { recursive: true })
    fs.writeFileSync(local, '')
    expect(selectBinClient(root, { pathValue: path.dirname(installed) }).source).toBe('build')
  })

  it('compiles only through an explicit --build, and keeps standalone unbound', () => {
    expect(releaseBuildScript('standalone-client')).toBe('scripts/tauri-build.mjs')
    expect(releaseBuildScript('managed-server')).toBe('scripts/build-server.mjs')

    const standalone = releaseBuildInvocation('standalone-client', {
      POINTER_EDITION: 'managed',
      POINTER_API_BASE: 'https://leak.example',
      POINTER_SERVER_ADMIN_PASSWORD_HMAC: 'secret',
      PATH: '/usr/bin',
    })
    expect(standalone.args[0]).toBe(path.join(REPO_ROOT, 'scripts/tauri-build.mjs'))
    expect(standalone.env.POINTER_EDITION).toBeUndefined()
    expect(standalone.env.POINTER_API_BASE).toBeUndefined()
    expect(standalone.env.POINTER_SERVER_ADMIN_PASSWORD_HMAC).toBeUndefined()
    expect(standalone.env.PATH).toBe('/usr/bin')
    expect(standalone.env.POINTER_LOCAL_ENV_KEEP_UNSET.split(',')).toEqual(STANDALONE_BUILD_KEEP_UNSET)

    const managed = releaseBuildInvocation('managed-client', {
      POINTER_EDITION: 'managed',
      POINTER_API_BASE: domains.apiBase,
      POINTER_WEB_BASE: domains.webBase,
      COMPUTER_ANNOTATE_API_BASE: domains.annotateBase,
    })
    expect(managed.args[0]).toBe(path.join(REPO_ROOT, 'scripts/tauri-build.mjs'))
    expect(managed.env.POINTER_EDITION).toBe('managed')
    expect(managed.env.POINTER_API_BASE).toBe(domains.apiBase)
    expect(managed.env.POINTER_LOCAL_ENV_KEEP_UNSET).toBeUndefined()

    const calls = []
    runReleaseBuild('standalone-server', { PATH: '/usr/bin' }, {
      spawnImpl: (command, args, options) => {
        calls.push({ command, args, options })
        return { status: 0 }
      },
    })
    expect(calls).toHaveLength(1)
    expect(calls[0].command).toBe(process.execPath)
    expect(calls[0].args[0]).toBe(path.join(REPO_ROOT, 'scripts/build-server.mjs'))
    expect(calls[0].options.stdio).toBe('inherit')
    expect(() => runReleaseBuild('standalone-client', {}, {
      spawnImpl: () => ({ status: 2 }),
    })).toThrow(/exited 2/)
  })

  it('leaves edition keys blank when a standalone build locks them', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-local-env-'))
    const file = path.join(dir, 'pointer.local.env')
    fs.writeFileSync(file, [
      'POINTER_EDITION=managed',
      'POINTER_API_BASE=https://leak.example',
      'VITE_POINTER_EDITION=managed',
      'OTHER_KEY=kept',
    ].join('\n'))
    const env = {
      POINTER_LOCAL_ENV_KEEP_UNSET: STANDALONE_BUILD_KEEP_UNSET.join(','),
    }
    applyPointerLocalEnv(env, { filePath: file, logPrefix: '[test]' })
    expect(env.POINTER_EDITION).toBeUndefined()
    expect(env.POINTER_API_BASE).toBeUndefined()
    expect(env.VITE_POINTER_EDITION).toBeUndefined()
    expect(env.OTHER_KEY).toBe('kept')
  })

  it('refuses a dev launch when the local vite or tauri cli is missing', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-quick-deploy-dev-'))
    expect(missingLaunchArtifact(plan('standalone-server', { root }))).toMatch(/vite\.js/)
    expect(missingLaunchArtifact(plan('standalone-client', { root }))).toMatch(/tauri\.js/)
  })
})
