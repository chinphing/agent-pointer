import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EffectiveSettingsView, UserSettings } from '../types/chat'

const updateUserSettings = vi.hoisted(() => vi.fn())
const getSettings = vi.hoisted(() => vi.fn())

vi.mock('../lib/theme', () => ({ applyTheme: vi.fn() }))
vi.mock('../lib/api', () => ({
  getSettings,
  updateUserSettings,
  updatePlatformSettings: vi.fn(),
  updateDebugSessionSettings: vi.fn(),
  setApiKey: vi.fn(),
  clearApiKey: vi.fn(),
  testConnection: vi.fn()
}))

import {
  DEFAULT_MAX_CHILDREN_PER_AGENT,
  MAX_MAX_CHILDREN_PER_AGENT,
  MIN_MAX_CHILDREN_PER_AGENT,
  clampMaxChildrenPerAgent,
  useSettingsStore
} from './settings'

function jsonClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

describe('settings maxChildrenPerAgent', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('clamps the fan-out cap to 1–32 with a default of 8', () => {
    expect(DEFAULT_MAX_CHILDREN_PER_AGENT).toBe(8)
    expect(clampMaxChildrenPerAgent(undefined)).toBe(8)
    expect(clampMaxChildrenPerAgent(Number.NaN)).toBe(8)
    expect(clampMaxChildrenPerAgent(0)).toBe(MIN_MAX_CHILDREN_PER_AGENT)
    expect(clampMaxChildrenPerAgent(1)).toBe(1)
    expect(clampMaxChildrenPerAgent(8)).toBe(8)
    expect(clampMaxChildrenPerAgent(99)).toBe(MAX_MAX_CHILDREN_PER_AGENT)
  })

  it('defaults the merged view to 8 and normalizes out-of-range stored values', async () => {
    const store = useSettingsStore()
    expect(store.settings.maxChildrenPerAgent).toBe(DEFAULT_MAX_CHILDREN_PER_AGENT)

    const view: EffectiveSettingsView = {
      user: jsonClone(store.userSettings),
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), maxChildrenPerAgent: 99 },
      canEditPlatform: true,
      isPlatformAdmin: true
    }
    getSettings.mockResolvedValue(view)
    await store.load()
    expect(store.settings.maxChildrenPerAgent).toBe(MAX_MAX_CHILDREN_PER_AGENT)
  })

  it('round-trips the fan-out cap through saveAgentPreferences', async () => {
    const store = useSettingsStore()
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => ({
      user: { ...jsonClone(store.userSettings), ...snapshot },
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), ...snapshot },
      canEditPlatform: true,
      isPlatformAdmin: true
    }))

    await store.saveAgentPreferences({ maxChildrenPerAgent: 3 })

    expect(store.settings.maxChildrenPerAgent).toBe(3)
    expect(store.userSettings.maxChildrenPerAgent).toBe(3)
    expect(updateUserSettings).toHaveBeenCalledWith(
      expect.objectContaining({ maxChildrenPerAgent: 3 })
    )
  })
})
