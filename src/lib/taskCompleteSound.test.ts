import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import {
  playTaskCompleteSound,
  playTaskCompleteSoundIfEnabled,
  resetTaskCompleteSoundStateForTests
} from './taskCompleteSound'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined)
}))

vi.mock('./runtime', () => ({
  isTauriRuntime: () => true
}))

vi.mock('../stores/settings', () => ({
  useSettingsStore: () => ({
    userSettings: { playSoundOnFinish: true }
  })
}))

describe('taskCompleteSound', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    resetTaskCompleteSoundStateForTests()
    vi.useFakeTimers()
    vi.setSystemTime(1_000_000)
  })

  it('skips a second play for the same turn even after debounce window', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockClear()

    playTaskCompleteSoundIfEnabled('conv-1', 'user-a')
    await Promise.resolve()
    expect(invoke).toHaveBeenCalledTimes(1)

    vi.setSystemTime(1_000_000 + 2000)
    playTaskCompleteSoundIfEnabled('conv-1', 'user-a')
    await Promise.resolve()
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('allows a different turn after debounce', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockClear()

    playTaskCompleteSoundIfEnabled('conv-1', 'user-a')
    await Promise.resolve()
    expect(invoke).toHaveBeenCalledTimes(1)

    vi.setSystemTime(1_000_000 + 2000)
    playTaskCompleteSoundIfEnabled('conv-1', 'user-b')
    await Promise.resolve()
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  it('time-debounces rapid plays without turn id', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockClear()

    await playTaskCompleteSound()
    expect(invoke).toHaveBeenCalledTimes(1)

    vi.setSystemTime(1_000_000 + 200)
    await playTaskCompleteSound()
    expect(invoke).toHaveBeenCalledTimes(1)
  })
})
