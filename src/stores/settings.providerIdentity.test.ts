import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EffectiveSettingsView } from '../types/chat'

vi.mock('../lib/theme', () => ({ applyTheme: vi.fn() }))
vi.mock('../lib/api', () => ({
  getSettings: vi.fn(),
  updateUserSettings: vi.fn(),
  updatePlatformSettings: vi.fn(),
  updateDebugSessionSettings: vi.fn(),
  setApiKey: vi.fn(),
  clearApiKey: vi.fn(),
  testConnection: vi.fn()
}))

import { useSettingsStore } from './settings'

describe('provider identity is (source, id)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('updates only the row with the matching source', () => {
    const store = useSettingsStore()
    store.settings.providers = [
      { id: 'deepseek', source: 'user', name: 'DeepSeek', baseUrl: 'https://a/v1', apiKey: 'sk-user', models: ['m'] },
      { id: 'deepseek', source: 'platform', name: 'DeepSeek', baseUrl: 'https://a/v1', apiKey: '', models: ['m'] }
    ] as never

    store.updateProvider('deepseek', { name: 'Renamed' } as never, 'user')

    const rows = store.settings.providers as Array<{ name: string; source?: string }>
    expect(rows[0].name).toBe('Renamed')
    expect(rows[1].name).toBe('DeepSeek')
  })

  it('collapses duplicate rows written by older builds and keeps the keyed one', () => {
    const store = useSettingsStore()
    const platform = JSON.parse(JSON.stringify(store.platformSettings))
    const view = {
      user: {},
      platform,
      merged: {
        providers: [
          { id: 'deepseek', source: 'user', name: 'DeepSeek', baseUrl: 'https://a/v1', apiKey: '', models: ['m'] },
          { id: 'deepseek', source: 'user', name: 'DeepSeek', baseUrl: 'https://a/v1', apiKey: 'sk-kept', models: ['m'] }
        ]
      },
      canEditPlatform: true,
      isPlatformAdmin: true
    } as unknown as EffectiveSettingsView

    store.applyEffectiveView(view)

    const deepseek = (store.settings.providers as Array<{ id: string; source?: string; apiKey?: string }>)
      .filter(p => p.id === 'deepseek' && (p.source ?? 'user') === 'user')
    expect(deepseek).toHaveLength(1)
    expect(deepseek[0].apiKey).toBe('sk-kept')
  })
})
