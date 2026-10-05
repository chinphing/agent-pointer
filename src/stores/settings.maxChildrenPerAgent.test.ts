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
  clampMaxChildrenPerAgent,
  defaultMaxChildrenPerAgent,
  maxChildrenPerAgentCeiling,
  maxChildrenPerAgentCeilingForCores,
  MIN_MAX_CHILDREN_PER_AGENT,
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

  it('derives the ceiling as clamp(cores × 10, 32, 100)', () => {
    // Mirrors Rust `ceiling_formula_is_cores_times_ten_clamped_to_thirty_two_and_one_hundred`.
    expect(maxChildrenPerAgentCeilingForCores(1)).toBe(32)
    expect(maxChildrenPerAgentCeilingForCores(2)).toBe(32)
    expect(maxChildrenPerAgentCeilingForCores(3)).toBe(32)
    expect(maxChildrenPerAgentCeilingForCores(4)).toBe(40)
    expect(maxChildrenPerAgentCeilingForCores(8)).toBe(80)
    expect(maxChildrenPerAgentCeilingForCores(10)).toBe(100)
    // Hard cap: more cores can never raise the ceiling above 100.
    expect(maxChildrenPerAgentCeilingForCores(16)).toBe(100)
    expect(maxChildrenPerAgentCeilingForCores(64)).toBe(100)
    expect(maxChildrenPerAgentCeilingForCores(1024)).toBe(100)
    // Same deterministic fallback as the Rust side when the host cannot report.
    expect(maxChildrenPerAgentCeilingForCores(0)).toBe(32)
    expect(maxChildrenPerAgentCeilingForCores(Number.NaN)).toBe(32)
    expect(maxChildrenPerAgentCeilingForCores(-4)).toBe(32)
  })

  it('uses the reported core count and defaults to the ceiling', () => {
    const ceiling = maxChildrenPerAgentCeiling()
    expect(ceiling).toBeGreaterThanOrEqual(32)
    expect(ceiling).toBeLessThanOrEqual(100)
    expect(defaultMaxChildrenPerAgent()).toBe(ceiling)

    expect(clampMaxChildrenPerAgent(undefined)).toBe(ceiling)
    expect(clampMaxChildrenPerAgent(Number.NaN)).toBe(ceiling)
    expect(clampMaxChildrenPerAgent(0)).toBe(MIN_MAX_CHILDREN_PER_AGENT)
    expect(clampMaxChildrenPerAgent(1)).toBe(1)
    expect(clampMaxChildrenPerAgent(8)).toBe(8)
    expect(clampMaxChildrenPerAgent(ceiling)).toBe(ceiling)
    expect(clampMaxChildrenPerAgent(ceiling + 1)).toBe(ceiling)
    // Regression for the reported bug: 50 is only cut when the host really has
    // fewer than 5 logical cores — the old hard cap of 32 is gone.
    expect(clampMaxChildrenPerAgent(50)).toBe(Math.min(50, ceiling))
    // A value stored before the hard cap existed (e.g. 160 on a 16-core host) is
    // pulled down to the ceiling on the read path.
    expect(clampMaxChildrenPerAgent(160)).toBe(ceiling)
    expect(clampMaxChildrenPerAgent(160)).toBeLessThanOrEqual(100)
  })

  it('defaults the merged view to the ceiling and clamps out-of-range stored values', async () => {
    const store = useSettingsStore()
    const ceiling = maxChildrenPerAgentCeiling()
    expect(store.settings.maxChildrenPerAgent).toBe(ceiling)

    const view: EffectiveSettingsView = {
      user: jsonClone(store.userSettings),
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), maxChildrenPerAgent: ceiling + 1 },
      canEditPlatform: true,
      isPlatformAdmin: true
    }
    getSettings.mockResolvedValue(view)
    await store.load()
    expect(store.settings.maxChildrenPerAgent).toBe(ceiling)
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

  it('round-trips a value above the old fixed cap (the reported bug)', async () => {
    const store = useSettingsStore()
    const aboveOldCap = MIN_MAX_CHILDREN_PER_AGENT + 32 // 33 > the old ceiling of 32
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => ({
      user: { ...jsonClone(store.userSettings), ...snapshot },
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), ...snapshot },
      canEditPlatform: true,
      isPlatformAdmin: true
    }))

    await store.saveAgentPreferences({ maxChildrenPerAgent: aboveOldCap })

    expect(store.userSettings.maxChildrenPerAgent).toBe(aboveOldCap)
    expect(store.settings.maxChildrenPerAgent).toBe(aboveOldCap)
  })
})
