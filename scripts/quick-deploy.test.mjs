import { describe, expect, it } from 'vitest'
import os from 'node:os'
import path from 'node:path'
import { buildPlan, localAdminCredentials, modeKind, parseArgs } from './quick-deploy.mjs'

const domains = {
  apiBase: 'https://pointer-api.example.com',
  webBase: 'https://pointer.example.com',
  annotateBase: 'https://pointer-som.example.com',
}

function plan(mode, extra = {}) {
  return buildPlan({
    mode,
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
    expect(result.processes[1].args).toEqual(['run', 'web:dev'])
  })

  it('standalone-client starts tauri dev without a deployment mode', () => {
    const result = plan('standalone-client')
    expect(result.authMode).toBeNull()
    expect(result.processes).toHaveLength(1)
    expect(result.processes[0].args[0]).toBe('tauri')
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
    expect(result.processes[0].command).toBe('npx')
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
  })
})
