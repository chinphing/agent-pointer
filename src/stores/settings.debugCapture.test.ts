import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

const getSettings = vi.fn()

vi.mock('../lib/api', () => ({
  getSettings: () => getSettings(),
  updateUserSettings: vi.fn(),
  saveProviderKey: vi.fn(),
  clearApiKey: vi.fn()
}))

// `applyEffectiveView` calls `applyTheme`, which touches `document`. The store
// logic under test has nothing to do with theming.
vi.mock('../lib/theme', () => ({
  applyTheme: vi.fn(),
  resolveThemeMode: () => 'light'
}))

vi.mock('../lib/i18n', () => ({
  applyUiLocale: vi.fn(),
  normalizeUiLocalePreference: (v: unknown) => v ?? 'system'
}))

const view = (mergedExtra: Record<string, unknown>) => ({
  user: { theme: 'system', uiLocale: 'system' },
  platform: {},
  merged: {
    providers: [],
    activeProviderId: '',
    model: '',
    ...mergedExtra
  },
  canEditPlatform: true,
  isPlatformAdmin: true
})

describe('settings store — debug capture switch', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    getSettings.mockReset()
  })

  it('keeps debugSaveComputerCaptures from the merged view', async () => {
    getSettings.mockResolvedValue(view({ debugSaveComputerCaptures: true }))
    const { useSettingsStore } = await import('./settings')
    const s = useSettingsStore()
    await s.load()
    expect(s.settings.debugSaveComputerCaptures).toBe(true)
  })

  it('falls back to the previous value when a later response omits it', async () => {
    getSettings.mockResolvedValue(view({ debugSaveComputerCaptures: true }))
    const { useSettingsStore } = await import('./settings')
    const s = useSettingsStore()
    await s.load()

    // A non-admin web response omits debug fields entirely.
    getSettings.mockResolvedValue(view({}))
    await s.load()
    expect(s.settings.debugSaveComputerCaptures).toBe(true)
  })
})
