// @vitest-environment happy-dom
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

vi.mock('./runtime', () => ({
  isTauriRuntime: vi.fn(() => false)
}))
import { i18n } from '../i18n'
import {
  DEFAULT_BRAND_ICON,
  DEFAULT_BRAND_NAME,
  resolveBrandIcon,
  resolveBrandName,
  resolveComposerPlaceholder,
  resolveDesktopSnapshotEnabled,
  resolveTurnElapsedPrefix,
  resolveWelcomeTip
} from './webBranding'
import { formatTurnElapsed } from './turnElapsed'
import { isTauriRuntime } from './runtime'

beforeAll(() => {
  i18n.global.locale.value = 'zh-CN'
})

function setMeta(name: string, content: string) {
  let el = document.querySelector(`meta[name="${name}"]`)
  if (!el) {
    el = document.createElement('meta')
    el.setAttribute('name', name)
    document.head.appendChild(el)
  }
  el.setAttribute('content', content)
}

function clearMeta(name: string) {
  document.querySelector(`meta[name="${name}"]`)?.remove()
}

afterEach(() => {
  clearMeta('pointer-welcome-tip-title')
  clearMeta('pointer-welcome-tip-body')
  clearMeta('pointer-turn-elapsed-active')
  clearMeta('pointer-turn-elapsed-done')
  clearMeta('pointer-brand-name')
  clearMeta('pointer-brand-icon')
  clearMeta('pointer-desktop-snapshot')
  clearMeta('pointer-composer-placeholder')
  vi.mocked(isTauriRuntime).mockReturnValue(false)
  vi.unstubAllEnvs()
  i18n.global.locale.value = 'zh-CN'
})

describe('web branding tip / elapsed', () => {
  it('returns null welcome tip when unset', () => {
    expect(resolveWelcomeTip()).toBeNull()
  })

  it('reads welcome tip from meta', () => {
    setMeta('pointer-welcome-tip-title', '我是财务报销助手')
    setMeta('pointer-welcome-tip-body', '预计 10–30 分钟')
    expect(resolveWelcomeTip()).toEqual({
      title: '我是财务报销助手',
      body: '预计 10–30 分钟'
    })
  })

  it('defaults turn elapsed prefix from locale catalog', () => {
    expect(resolveTurnElapsedPrefix('active')).toBe('工作')
    expect(resolveTurnElapsedPrefix('done')).toBe('工作')
  })

  it('reads active / done prefixes from meta', () => {
    setMeta('pointer-turn-elapsed-active', '报销单填写中')
    setMeta('pointer-turn-elapsed-done', '报销单已填写')
    expect(resolveTurnElapsedPrefix('active')).toBe('报销单填写中')
    expect(resolveTurnElapsedPrefix('done')).toBe('报销单已填写')
  })

  it('ignores stock Chinese composer meta when locale is en', () => {
    setMeta('pointer-composer-placeholder', '告诉我你想做什么')
    i18n.global.locale.value = 'en'
    expect(resolveComposerPlaceholder()).toBe("Tell me what you'd like to do")
  })

  it('keeps custom white-label composer placeholder', () => {
    setMeta('pointer-composer-placeholder', 'Upload an invoice…')
    i18n.global.locale.value = 'en'
    expect(resolveComposerPlaceholder()).toBe('Upload an invoice…')
  })
})

describe('formatTurnElapsed branding', () => {
  it('keeps product defaults without meta', () => {
    expect(formatTurnElapsed(125_999)).toBe('工作 2 m 05 s')
    expect(formatTurnElapsed(900)).toBe('工作 0 m 00 s')
    expect(formatTurnElapsed(null)).toBe('工作耗时未知')
  })

  it('uses custom prefixes including unknown', () => {
    setMeta('pointer-turn-elapsed-active', '报销单填写中')
    setMeta('pointer-turn-elapsed-done', '报销单已填写')
    expect(formatTurnElapsed(125_999, 'active')).toBe('报销单填写中 2 m 05 s')
    expect(formatTurnElapsed(null, 'done')).toBe('报销单已填写耗时未知')
  })

  it('switches work prefix with English locale', () => {
    i18n.global.locale.value = 'en'
    expect(formatTurnElapsed(125_999)).toBe('Work 2 m 05 s')
    expect(formatTurnElapsed(null)).toBe('Work · time unknown')
  })
})

describe('web branding name / icons / snapshot', () => {
  it('defaults brand name and icon', () => {
    expect(resolveBrandName()).toBe(DEFAULT_BRAND_NAME)
    expect(resolveBrandIcon()).toBe(DEFAULT_BRAND_ICON)
  })

  it('hides desktop snapshot when unset', () => {
    expect(resolveDesktopSnapshotEnabled()).toBe(false)
  })

  it('shows desktop snapshot when meta is 1', () => {
    setMeta('pointer-desktop-snapshot', '1')
    expect(resolveDesktopSnapshotEnabled()).toBe(true)
  })

  it('reads brand name and icon from meta', () => {
    setMeta('pointer-brand-name', '财务助手')
    setMeta('pointer-brand-icon', '/branding/logo.png')
    expect(resolveBrandName()).toBe('财务助手')
    expect(resolveBrandIcon()).toBe('/branding/logo.png')
  })

  it('hides desktop snapshot when meta is 0', () => {
    setMeta('pointer-desktop-snapshot', '0')
    expect(resolveDesktopSnapshotEnabled()).toBe(false)
  })

  it('hides desktop snapshot in the desktop app even when enabled', () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true)
    vi.stubEnv('VITE_DESKTOP_SNAPSHOT_ENABLED', '1')
    setMeta('pointer-desktop-snapshot', '1')
    expect(resolveDesktopSnapshotEnabled()).toBe(false)
  })
})
