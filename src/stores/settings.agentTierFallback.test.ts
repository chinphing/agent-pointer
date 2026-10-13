import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentPerformanceModes, EffectiveSettingsView } from '../types/chat'

const getSettings = vi.hoisted(() => vi.fn())

vi.mock('../lib/theme', () => ({ applyTheme: vi.fn() }))
vi.mock('../lib/api', () => ({
  getSettings,
  updateUserSettings: vi.fn(),
  updatePlatformSettings: vi.fn(),
  updateDebugSessionSettings: vi.fn(),
  setApiKey: vi.fn(),
  clearApiKey: vi.fn(),
  testConnection: vi.fn()
}))

import { useSettingsStore } from './settings'

function jsonClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

describe('settings agent tier fallback mirror', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('defaults agent tiers to standard while media stays fast', () => {
    const store = useSettingsStore()
    // Local seed mirrors the backend agent fallback (`standard`).
    expect(store.getAgentPerformanceMode('general')).toBe('standard')
    expect(store.getAgentPerformanceMode('coder')).toBe('standard')
    // An agent with no stored value still falls back to `standard` …
    expect(store.getAgentPerformanceMode('explore')).toBe('standard')
    // … while the non-agent media scenes keep `fast`.
    expect(store.getMediaUnderstandingMode('image')).toBe('fast')
    expect(store.getMediaUnderstandingMode('audio')).toBe('fast')
    expect(store.getMediaUnderstandingMode('video')).toBe('fast')
  })

  it('keeps stored explicit agent tiers when the effective view loads', async () => {
    const store = useSettingsStore()
    // The user's persisted values; the backend `merged` slice mirrors the user
    // slice for this field, so the local seed must not clobber either one.
    const stored: AgentPerformanceModes = {
      general: 'standard',
      coder: 'fast',
      explore: 'standard'
    }
    getSettings.mockResolvedValue({
      user: { ...jsonClone(store.userSettings), agentPerformanceModes: stored },
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), agentPerformanceModes: stored },
      canEditPlatform: true,
      isPlatformAdmin: true
    } satisfies EffectiveSettingsView)

    await store.load()

    expect(store.getAgentPerformanceMode('general')).toBe('standard')
    expect(store.getAgentPerformanceMode('coder')).toBe('fast')
    expect(store.getAgentPerformanceMode('explore')).toBe('standard')
  })
})
