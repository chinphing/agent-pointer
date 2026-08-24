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

describe('settings parallel preference round-trip', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('writes maxParallelSubAgents back onto merged settings after save', async () => {
    const store = useSettingsStore()
    store.settings.maxParallelSubAgents = 8
    store.userSettings.maxParallelSubAgents = 8
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => {
      const view: EffectiveSettingsView = {
        user: { ...jsonClone(store.userSettings), ...snapshot },
        platform: jsonClone(store.platformSettings),
        merged: {
          ...jsonClone(store.settings),
          maxParallelSubAgents: snapshot.maxParallelSubAgents,
          maxParallelToolCalls: snapshot.maxParallelToolCalls,
          maxParallelMediaJobs: snapshot.maxParallelMediaJobs,
          parallelToolExecutionEnabled: snapshot.parallelToolExecutionEnabled,
          maxConcurrentRuns: snapshot.maxConcurrentRuns
        },
        canEditPlatform: true,
        isPlatformAdmin: true
      }
      return view
    })

    await store.saveAgentPreferences({ maxParallelSubAgents: 2 })

    expect(store.settings.maxParallelSubAgents).toBe(2)
    expect(store.userSettings.maxParallelSubAgents).toBe(2)
  })

  it('writes execution and content-limit fields back onto merged settings after save', async () => {
    const store = useSettingsStore()
    store.settings.maxConcurrentRuns = 4
    store.settings.parallelToolExecutionEnabled = true
    store.settings.maxParallelToolCalls = 8
    store.settings.maxParallelMediaJobs = 8
    store.settings.fileGrepMaxResults = 50
    store.settings.fileReadMaxBytes = 65_536
    store.settings.fileLineMaxBytes = 1024
    store.settings.terminalOutputMaxBytes = 16_384
    store.settings.terminalTimeoutSeconds = 30
    store.settings.terminalMaxWallHours = 24
    store.settings.attachmentUploadMaxBytes = 100 * 1024 * 1024
    store.settings.toolApprovalMode = 'auto'
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => {
      const view: EffectiveSettingsView = {
        user: { ...jsonClone(store.userSettings), ...snapshot },
        platform: jsonClone(store.platformSettings),
        merged: { ...jsonClone(store.settings), ...snapshot },
        canEditPlatform: true,
        isPlatformAdmin: true
      }
      return view
    })

    await store.saveAgentPreferences({
      maxConcurrentRuns: 2,
      parallelToolExecutionEnabled: false,
      maxParallelToolCalls: 3,
      maxParallelMediaJobs: 1,
      fileGrepMaxResults: 10,
      fileReadMaxBytes: 8192,
      fileLineMaxBytes: 512,
      terminalOutputMaxBytes: 4096,
      terminalTimeoutSeconds: 120,
      terminalMaxWallHours: 2,
      attachmentUploadMaxBytes: 20 * 1024 * 1024,
      toolApprovalMode: 'manual'
    })

    expect(store.settings.maxConcurrentRuns).toBe(2)
    expect(store.settings.parallelToolExecutionEnabled).toBe(false)
    expect(store.settings.maxParallelToolCalls).toBe(3)
    expect(store.settings.maxParallelMediaJobs).toBe(1)
    expect(store.settings.fileGrepMaxResults).toBe(10)
    expect(store.settings.fileReadMaxBytes).toBe(8192)
    expect(store.settings.fileLineMaxBytes).toBe(512)
    expect(store.settings.terminalOutputMaxBytes).toBe(4096)
    expect(store.settings.terminalTimeoutSeconds).toBe(120)
    expect(store.settings.terminalMaxWallHours).toBe(2)
    expect(store.settings.attachmentUploadMaxBytes).toBe(20 * 1024 * 1024)
    expect(store.settings.toolApprovalMode).toBe('manual')
  })

  it('does not drop content limits when saving an unrelated user patch', async () => {
    const store = useSettingsStore()
    store.userSettings.fileGrepMaxResults = 10
    store.userSettings.fileReadMaxBytes = 8192
    store.userSettings.maxConcurrentRuns = 2
    store.userSettings.maxParallelSubAgents = 3
    updateUserSettings.mockImplementation(async (snapshot: UserSettings) => ({
      user: snapshot,
      platform: jsonClone(store.platformSettings),
      merged: { ...jsonClone(store.settings), ...snapshot },
      canEditPlatform: true,
      isPlatformAdmin: true
    }))

    await store.saveUser({ theme: 'dark' })

    expect(updateUserSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        fileGrepMaxResults: 10,
        fileReadMaxBytes: 8192,
        maxConcurrentRuns: 2,
        maxParallelSubAgents: 3,
        theme: 'dark'
      })
    )
  })

  it('clears maxParallelSubAgents when the saved view omits auto (null)', async () => {
    const store = useSettingsStore()
    store.settings.maxParallelSubAgents = 2
    store.userSettings.maxParallelSubAgents = 2
    updateUserSettings.mockImplementation(async () => {
      const user = jsonClone(store.userSettings)
      delete user.maxParallelSubAgents
      const merged = jsonClone(store.settings)
      delete merged.maxParallelSubAgents
      const view: EffectiveSettingsView = {
        user,
        platform: jsonClone(store.platformSettings),
        merged,
        canEditPlatform: true,
        isPlatformAdmin: true
      }
      return view
    })

    await store.saveAgentPreferences({ maxParallelSubAgents: null })

    expect(store.settings.maxParallelSubAgents).toBeNull()
  })
})
