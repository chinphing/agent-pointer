// @vitest-environment happy-dom

import { createApp, nextTick, reactive, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentTrace, ChatMessage } from '../../../../types/chat'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { i18n } from '../../../../i18n'
import { applyUiLocale } from '../../../../lib/uiLocale'
import {
  resetConversationScopedStoreForTests,
  useConversationScopedStore
} from '../../../../lib/conversationScoped'
import { renderPerfSnapshot, setRenderPerfEnabled } from '../../../../lib/renderPerf'
import SubAgentFrameHost from './SubAgentFrameHost.vue'

/**
 * The host's live subscription must live in `SubAgentFrame`, not in the host.
 *
 * Render counts come from the app's own instrumentation (`render:*` counters), which
 * is exactly the metric the HUD reports, so the assertion is the same number the
 * real-device sample reads.
 */
const hoisted = vi.hoisted(() => ({
  live: null as Record<string, string> | null
}))

vi.mock('../../../../stores/chat', async () => {
  const { reactive: reactiveFn } = await import('vue')
  const { useConversationScopedStore: scopedStore } = await import(
    '../../../../lib/conversationScoped'
  )
  hoisted.live = reactiveFn<Record<string, string>>({})
  const chat = {
    currentId: 'c1' as string | null,
    current: { id: 'c1', messages: [] as ChatMessage[] } as {
      id: string
      messages: ChatMessage[]
      workspaceRoot?: string
    } | null,
    contextCompressing: null as unknown,
    terminalLiveViewReadyToolCallId: null as string | null,
    getSubAgentLiveSignal: (traceId: string) => hoisted.live![traceId?.trim()] ?? '',
    scopedMessagesForTraceCached: (
      anchorMessageId: string,
      traceId: string,
      agentInstanceId?: string
    ) => {
      const convId = (chat.currentId ?? '').trim()
      if (!convId) return []
      // Mirrors the real store: the live fingerprint is the reactive edge that makes
      // a streamed scoped row visible.
      if (agentInstanceId) void hoisted.live![agentInstanceId]
      void hoisted.live![traceId]
      return scopedStore()
        .getRows(convId, { anchorMessageId, traceId, agentInstanceId })
        .slice()
    },
    ensureScopedMessagesForTrace: async () => {},
    ensureMessageAside: async () => {},
    ensureToolCallBody: async () => {},
    releaseToolCallBody: () => {},
    openTerminalLivePopup: () => {},
    evictScopedInstance: (
      convId: string,
      lookup: { anchorMessageId?: string | null; traceId?: string | null; agentInstanceId?: string | null },
      reason: 'stub' | 'trim' | 'manual' | 'conversation'
    ) => {
      scopedStore().evictInstance(convId, lookup, reason)
    }
  }
  return { useChatStore: () => chat }
})

vi.mock('../../../../stores/settings', () => ({
  useSettingsStore: () => ({ settings: { rawContentViewEnabled: false } })
}))

vi.mock('../../../../composables/useAgentUi', () => ({
  useAgentsCatalog: () => ({ value: [] })
}))

const messageUi: ResolvedAgentUi = {
  showInComposer: false,
  showSidecarToolCalls: false,
  showNonSidecarToolCalls: true,
  showReasoning: false,
  showSubAgentTrace: true,
  showToolCalls: true,
  showToolCallResults: true,
  hideToolNames: [],
  showWorkspacePicker: false,
  showComputerMonitorPicker: false,
  showTaskBoardPanel: false,
  userSelectable: false,
  composerLabel: 'coder',
  avatar: ''
}

function runningTrace(): AgentTrace {
  return {
    id: 'inst-1',
    name: 'coder',
    role: 'worker',
    status: 'running',
    depth: 1,
    agentInstanceId: 'inst-1',
    anchorMessageId: 'lead',
    // Expanded so the frame paints its per-round content blocks, which is what the
    // live test asserts on.
    userExpanded: true
  }
}

function scopedRow(id: string, over: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 1,
    anchorMessageId: 'lead',
    traceId: 'inst-1',
    agentInstanceId: 'inst-1',
    ...over
  }
}

const mountedApps: App[] = []

function mountHost(traceValue: AgentTrace): HTMLElement {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(SubAgentFrameHost, {
    trace: reactive(traceValue),
    anchorMessageId: 'lead',
    messages: [],
    messageUi,
    createdAt: 1,
    generating: true,
    isActiveGenerationMessage: false,
    showMessageActions: false,
    taskBoard: null,
    hostTool: null
  })
  app.use(i18n)
  mountedApps.push(app)
  app.mount(host)
  return host
}

async function flush() {
  for (let i = 0; i < 4; i += 1) await nextTick()
}

beforeEach(() => {
  applyUiLocale('zh-CN')
  resetConversationScopedStoreForTests()
})

afterEach(() => {
  setRenderPerfEnabled(false, { persist: false })
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

/**
 * A background spawn's only record is the host `run_subagent` row plus the running
 * `agent_step` — there are no scoped rows until the job streams. The frame still has to
 * paint, and its stats line has to read that host row.
 */
describe('background spawn frame', () => {
  it('paints under the host run_subagent row while the job runs', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('r1', {
        toolCalls: [{ id: 'call-bg', name: 'run_subagent', arguments: '{}', status: 'success' }]
      })
    ])
    const running = mountHost({ ...runningTrace(), parentToolCallId: 'call-bg' })
    await flush()

    // Frame, not stub — the job is still running and owns the host row.
    expect(running.querySelectorAll('.sub-agent-frame')).toHaveLength(1)
    expect(running.querySelector('.sub-agent-frame-stub')).toBeNull()

    // Terminal step keeps the same frame, and its stats line reads the host row's tool call.
    const finished = mountHost({
      ...runningTrace(),
      status: 'completed',
      parentToolCallId: 'call-bg'
    })
    await flush()

    expect(finished.querySelectorAll('.sub-agent-frame')).toHaveLength(1)
    expect(finished.textContent).toContain('委派 1 次')
  })
})

describe('SubAgentFrameHost live subscription', () => {
  it('a chunk of its own spawn re-renders the frame, not the host', async () => {
    setRenderPerfEnabled(true, { persist: false })
    useConversationScopedStore().ingestRows('c1', [scopedRow('r1', { content: '第一段' })])
    mountHost(runningTrace())
    await flush()

    // Establish the window, then close it: a mount is not an update.
    renderPerfSnapshot(1_000_000)
    const mounted = renderPerfSnapshot(1_001_000)
    expect(mounted.mounts['mount:SubAgentFrameHost']).toBe(1)

    // A streamed chunk lands for this spawn.
    hoisted.live!['inst-1'] = 'fp-1'
    await flush()

    const streamed = renderPerfSnapshot(1_002_000)
    expect(streamed.renders['render:SubAgentFrameHost'] ?? 0).toBe(0)
    expect(streamed.renders['render:SubAgentFrame'] ?? 0).toBeGreaterThan(0)
  })

  it('keeps the frame live: a new scoped row shows up without a host render', async () => {
    setRenderPerfEnabled(true, { persist: false })
    const store = useConversationScopedStore()
    store.ingestRows('c1', [scopedRow('r1', { content: '第一段' })])
    const host = mountHost(runningTrace())
    await flush()
    expect(host.querySelectorAll('[data-sub-agent-content-id]').length).toBe(1)

    // The worker streams a second round.
    store.ingestRows('c1', [scopedRow('r2', { content: '第二段', createdAt: 2 })])
    hoisted.live!['inst-1'] = 'fp-2'
    await flush()

    expect(host.querySelectorAll('[data-sub-agent-content-id]').length).toBe(2)
    renderPerfSnapshot(2_000_000)
    const streamed = renderPerfSnapshot(2_001_000)
    expect(streamed.renders['render:SubAgentFrameHost'] ?? 0).toBe(0)
    expect(streamed.renders['render:SubAgentFrame'] ?? 0).toBeGreaterThan(0)
  })

  it('another spawn streaming does not touch this host or frame', async () => {
    setRenderPerfEnabled(true, { persist: false })
    useConversationScopedStore().ingestRows('c1', [scopedRow('r1', { content: '第一段' })])
    mountHost(runningTrace())
    await flush()
    renderPerfSnapshot(3_000_000)

    hoisted.live!['inst-other'] = 'fp-other'
    await flush()

    const other = renderPerfSnapshot(3_001_000)
    expect(other.renders['render:SubAgentFrameHost'] ?? 0).toBe(0)
    expect(other.renders['render:SubAgentFrame'] ?? 0).toBe(0)
  })
})
