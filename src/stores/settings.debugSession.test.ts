import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EffectiveSettingsView } from '../types/chat'

const updateUserSettings = vi.hoisted(() => vi.fn())
const updateDebugSessionSettings = vi.hoisted(() => vi.fn())
const updateAgentSettings = vi.hoisted(() => vi.fn())

vi.mock('../lib/theme', () => ({ applyTheme: vi.fn() }))
vi.mock('../lib/api', () => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  updateAgentSettings,
  updateUserSettings,
  updatePlatformSettings: vi.fn(),
  updateDebugSessionSettings,
  setApiKey: vi.fn(),
  clearApiKey: vi.fn(),
  testConnection: vi.fn()
}))

import { useSettingsStore } from './settings'

function jsonClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

describe('settings debug-session save', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('keeps the selected model when theme save returns an old effective view', async () => {
    const store = useSettingsStore()
    const oldView: EffectiveSettingsView = {
      user: { theme: 'system' },
      platform: jsonClone(store.platformSettings),
      merged: jsonClone(store.settings),
      canEditPlatform: true,
      isPlatformAdmin: true
    }
    store.settings.model = 'qwen3.7-plus'
    const debugSnapshot = store.createDebugSessionSnapshot()

    updateUserSettings.mockResolvedValue(oldView)
    updateDebugSessionSettings.mockImplementation(async settings => settings)

    await store.saveUser({ theme: 'dark' })
    await store.saveDebugSession(debugSnapshot)

    expect(store.settings.model).toBe('qwen3.7-plus')
    expect(updateDebugSessionSettings).toHaveBeenCalledWith(
      expect.objectContaining({ model: 'qwen3.7-plus' })
    )
  })

  it('persists model service config through the agent-settings endpoint', async () => {
    const store = useSettingsStore()
    const view: EffectiveSettingsView = {
      user: jsonClone(store.userSettings),
      platform: jsonClone(store.platformSettings),
      merged: jsonClone(store.settings),
      canEditPlatform: true,
      isPlatformAdmin: true
    }
    updateAgentSettings.mockResolvedValue(view)

    await store.saveModelService({
      providers: store.settings.providers,
      activeProviderId: 'qwen',
      model: 'qwen3.5-plus',
      temperature: 0.7,
      maxTokens: 2048
    })

    expect(updateAgentSettings).toHaveBeenCalledWith(
      expect.objectContaining({ activeProviderId: 'qwen', model: 'qwen3.5-plus' })
    )
    expect(updateDebugSessionSettings).not.toHaveBeenCalled()
  })

  it('recomputes hasKey from the applied active provider', async () => {
    const store = useSettingsStore()
    const snapshot = store.createDebugSessionSnapshot()
    snapshot.providers[0].apiKey = '****'
    updateDebugSessionSettings.mockResolvedValue(snapshot)

    await store.saveDebugSession(snapshot)

    expect(store.settings.hasKey).toBe(true)
  })

  it('selects a valid model when removing the active provider', () => {
    const store = useSettingsStore()
    store.settings.activeProviderId = 'qwen'
    store.settings.model = 'qwen3.5-plus'

    store.removeProvider('qwen')

    const active = store.settings.providers.find(
      provider => provider.id === store.settings.activeProviderId
    )
    expect(active).toBeDefined()
    expect(active?.models).toContain(store.settings.model)
  })

  it('realigns active model when renaming models on the active provider', () => {
    const store = useSettingsStore()
    store.addProvider({
      id: 'custom-local',
      name: '本地',
      baseUrl: 'http://127.0.0.1:8080/v1',
      apiKey: 'test-key',
      models: ['old-model'],
      modelConfigs: {}
    })
    expect(store.settings.activeProviderId).toBe('custom-local')
    expect(store.settings.model).toBe('old-model')

    store.updateProvider('custom-local', {
      models: ['qwen3.6-27b-int8']
    })

    expect(store.settings.model).toBe('qwen3.6-27b-int8')
  })

  it('keeps the active provider when no replacement model exists', () => {
    const store = useSettingsStore()
    const activeId = store.settings.activeProviderId
    store.settings.providers = store.settings.providers.filter(
      provider => provider.id === activeId
    )

    store.removeProvider(activeId)

    expect(store.settings.providers).toHaveLength(1)
    expect(store.settings.activeProviderId).toBe(activeId)
  })
})
