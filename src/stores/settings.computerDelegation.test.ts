import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EffectiveSettingsView, UserSettings } from '../types/chat'

const updateUserSettings = vi.hoisted(() => vi.fn())

vi.mock('../lib/theme', () => ({ applyTheme: vi.fn() }))
vi.mock('../lib/api', () => ({
  getSettings: vi.fn(),
  updateUserSettings,
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

describe('computer delegation tier round-trip', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('keeps an explicit off after save instead of falling back to confirm', async () => {
    const store = useSettingsStore()
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => {
      const view: EffectiveSettingsView = {
        canEditPlatform: false,
        isPlatformAdmin: false,
        user: { ...jsonClone(store.userSettings), ...snapshot },
        platform: jsonClone(store.platformSettings),
        merged: {
          ...jsonClone(store.settings),
          computerDelegation: snapshot.computerDelegation
        }
      }
      return view
    })

    await store.saveUser({ computerDelegation: 'off' })

    expect(store.userSettings.computerDelegation).toBe('off')
    expect(store.settings.computerDelegation).toBe('off')
  })

  it('falls back to confirm when neither merged nor user carries the field', async () => {
    const store = useSettingsStore()
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => {
      const user = { ...jsonClone(store.userSettings), ...snapshot }
      delete user.computerDelegation
      const merged = { ...jsonClone(store.settings) }
      delete merged.computerDelegation
      const view: EffectiveSettingsView = {
        canEditPlatform: false,
        isPlatformAdmin: false,
        user,
        platform: jsonClone(store.platformSettings),
        merged
      }
      return view
    })

    await store.saveUser({ computerHumanLike: false })

    expect(store.settings.computerDelegation).toBe('confirm')
  })

  it('accepts auto and rejects unknown values at the merge boundary', async () => {
    const store = useSettingsStore()
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => {
      const view: EffectiveSettingsView = {
        canEditPlatform: false,
        isPlatformAdmin: false,
        user: { ...jsonClone(store.userSettings), ...snapshot },
        platform: jsonClone(store.platformSettings),
        merged: {
          ...jsonClone(store.settings),
          computerDelegation: snapshot.computerDelegation
        }
      }
      return view
    })

    await store.saveUser({ computerDelegation: 'auto' })
    expect(store.settings.computerDelegation).toBe('auto')

    // 后端理论上不会回未知值，但真回了也不能当成「自动」放行。
    updateUserSettings.mockImplementation(async () => {
      const view: EffectiveSettingsView = {
        canEditPlatform: false,
        isPlatformAdmin: false,
        user: jsonClone(store.userSettings),
        platform: jsonClone(store.platformSettings),
        merged: {
          ...jsonClone(store.settings),
          computerDelegation: 'bogus' as never
        }
      }
      return view
    })
    await store.saveUser({})
    expect(store.settings.computerDelegation).toBe('confirm')
  })
})
