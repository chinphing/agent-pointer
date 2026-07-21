import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DebugSessionSettings } from '../types/chat'

const invoke = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { updateDebugSessionSettings as updateDesktopDebugSession } from './tauri'
import { updateDebugSessionSettings as updateWebDebugSession } from './web'

const snapshot = {
  providers: [],
  activeProviderId: 'session-provider',
  model: 'session-model',
  temperature: 0.4,
  maxTokens: 4096,
  computerTierLlm: {},
  computerPipelineLlm: {},
  agentModeLlm: {},
  mediaModeLlm: {}
} satisfies DebugSessionSettings

describe('debug session settings transports', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.stubGlobal('window', globalThis)
  })

  it('uses the dedicated desktop command', async () => {
    invoke.mockResolvedValue(snapshot)

    await updateDesktopDebugSession(snapshot)

    expect(invoke).toHaveBeenCalledWith('update_debug_session_settings', {
      settings: snapshot
    })
  })

  it('uses the dedicated web endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify(snapshot), {
        status: 200,
        headers: { 'Content-Type': 'application/json' }
      })
    )
    vi.stubGlobal('fetch', fetchMock)

    await updateWebDebugSession(snapshot)

    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/debug-session-settings'),
      expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify(snapshot)
      })
    )
  })
})
