import { afterEach, describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {
  configuredDomains,
  configuredWebBase,
  effectiveEdition,
  fileContainsBytes,
  pathContains,
  verifyBakedEdition,
} from './verify-baked-edition.mjs'

const API_BASE = 'https://pointer-api.example.com'
const WEB_BASE = 'https://pointer.example.com'
const ANNOTATE_BASE = 'https://pointer-som.example.com'

const MANAGED_ENV = {
  POINTER_EDITION: 'managed',
  VITE_POINTER_EDITION: 'managed',
  POINTER_API_BASE: API_BASE,
  POINTER_WEB_BASE: WEB_BASE,
  VITE_POINTER_WEB_BASE: WEB_BASE,
  COMPUTER_ANNOTATE_API_BASE: ANNOTATE_BASE,
}

/** Stand-in for a real artifact: `text` is what a `strings`-style scan would find. */
function artifact(text) {
  return { probe: needle => text.includes(needle) }
}

function check({ env = MANAGED_ENV, binary = null, web = null, binaryProbe, webProbe } = {}) {
  return verifyBakedEdition({
    edition: effectiveEdition(env),
    domains: configuredDomains(env),
    webBase: configuredWebBase(env),
    binaryProbe: binaryProbe !== undefined ? binaryProbe : binary ? binary.probe : null,
    webProbe: webProbe !== undefined ? webProbe : web ? web.probe : null,
    binaryLabel: 'target/release/pointer-server',
    webLabel: 'dist/',
  })
}

const FULL_BINARY = artifact(`${API_BASE}\u0000${WEB_BASE}\u0000${ANNOTATE_BASE}`)
const MANAGED_WEB = artifact(`window.env={"VITE_POINTER_EDITION":"managed","VITE_POINTER_WEB_BASE":"${WEB_BASE}"}`)

const tempDirs = []

afterEach(() => {
  while (tempDirs.length > 0) {
    fs.rmSync(tempDirs.pop(), { recursive: true, force: true })
  }
})

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pointer-baked-edition-'))
  tempDirs.push(dir)
  return dir
}

describe('effectiveEdition', () => {
  it('accepts only `managed`; everything else is standalone', () => {
    expect(effectiveEdition({ POINTER_EDITION: ' managed ' })).toBe('managed')
    expect(effectiveEdition({ VITE_POINTER_EDITION: 'MANAGED' })).toBe('managed')
    expect(effectiveEdition({})).toBe('standalone')
    expect(effectiveEdition({ POINTER_EDITION: 'official' })).toBe('standalone')
  })
})

describe('verifyBakedEdition', () => {
  it('passes a fully bound managed build and reports both halves', () => {
    const result = check({ binary: FULL_BINARY, web: MANAGED_WEB })
    expect(result.errors).toEqual([])
    expect(result.warnings).toEqual([])
    expect(result.ok).toBe(true)
    expect(result.summary).toBe('edition=managed domains=3/3 baked, web=managed')
  })

  it('fails a half-bound build: managed web assets over a standalone binary', () => {
    const result = check({ binary: artifact('edition: unset (standalone defaults)'), web: MANAGED_WEB })
    expect(result.ok).toBe(false)
    expect(result.summary).toBe('edition=managed domains=0/3 baked, web=managed')
    expect(result.errors.join('\n')).toContain(API_BASE)
    expect(result.errors.join('\n')).toContain(ANNOTATE_BASE)
  })

  it('fails a managed build whose web assets lost the web base', () => {
    const result = check({ binary: FULL_BINARY, web: artifact('<html>no control plane here</html>') })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('dist/')
    expect(result.errors.join('\n')).toContain(WEB_BASE)
  })

  it('fails a managed build when the release binary is missing', () => {
    const result = check({ binaryProbe: null, web: MANAGED_WEB })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('target/release/pointer-server')
  })

  it('fails a managed build with no configured domains at all', () => {
    const result = check({
      env: { POINTER_EDITION: 'managed' },
      binary: artifact(''),
      web: artifact(''),
    })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('POINTER_WEB_BASE')
  })

  it('keeps a clean standalone build quiet', () => {
    const result = check({ env: {}, binary: artifact('edition: unset'), web: artifact('<html>') })
    expect(result.ok).toBe(true)
    expect(result.warnings).toEqual([])
    expect(result.summary).toBe('edition=standalone domains=0/0 baked, web=standalone')
  })

  it('warns (but does not fail) when a standalone binary carries a control-plane domain', () => {
    const result = check({
      env: { POINTER_API_BASE: API_BASE },
      binary: artifact(`built with ${API_BASE}`),
      web: artifact('<html>'),
    })
    expect(result.ok).toBe(true)
    expect(result.summary).toBe('edition=standalone domains=1/1 baked, web=standalone')
    expect(result.warnings).toHaveLength(1)
    expect(result.warnings[0]).toContain(API_BASE)
  })
})

describe('file probes', () => {
  it('finds a needle straddling a chunk boundary', () => {
    const dir = makeTempDir()
    const file = path.join(dir, 'pointer-server')
    const needle = 'POINTER_BUILTIN_API_BASE=https://pointer-api.example.com'
    fs.writeFileSync(file, Buffer.concat([Buffer.alloc(64 * 1024 - 4, 0x41), Buffer.from(needle)]))
    expect(fileContainsBytes(file, needle)).toBe(true)
    expect(fileContainsBytes(file, 'https://missing.example.com')).toBe(false)
  })

  it('scans a directory tree and reports a missing target', () => {
    const dir = makeTempDir()
    fs.mkdirSync(path.join(dir, 'assets'))
    fs.writeFileSync(path.join(dir, 'assets', 'index.js'), `const webBase = "${WEB_BASE}"`)
    expect(pathContains(dir, WEB_BASE)).toBe(true)
    expect(pathContains(dir, API_BASE)).toBe(false)
    expect(pathContains(path.join(dir, 'nope'), WEB_BASE)).toBeNull()
  })
})
