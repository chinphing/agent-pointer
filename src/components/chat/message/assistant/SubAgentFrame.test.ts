// @vitest-environment happy-dom

import { createApp, nextTick, reactive, ref, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentTrace, ChatMessage } from '../../../../types/chat'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { i18n } from '../../../../i18n'
import { applyUiLocale } from '../../../../lib/uiLocale'
import {
  resetConversationScopedStoreForTests,
  useConversationScopedStore
} from '../../../../lib/conversationScoped'
import SubAgentFrame from './SubAgentFrame.vue'

vi.mock('../../../../stores/chat', async () => {
  const { useConversationScopedStore: scopedStore } = await import(
    '../../../../lib/conversationScoped'
  )
  const chat = {
    currentId: 'c1' as string | null,
    current: { id: 'c1', messages: [] as ChatMessage[] } as {
      id: string
      messages: ChatMessage[]
      workspaceRoot?: string
    } | null,
    contextCompressing: null as unknown,
    terminalLiveViewReadyToolCallId: null as string | null,
    getSubAgentLiveSignal: () => '',
    ensureScopedMessagesForTrace: async () => {},
    ensureMessageAside: async () => {},
    ensureToolCallBody: async () => {},
    releaseToolCallBody: () => {},
    openTerminalLivePopup: () => {},
    // Real store semantics: the stub path frees that child's own scoped heap.
    evictScopedInstance: (
      convId: string,
      lookup: { anchorMessageId?: string | null; traceId?: string | null; agentInstanceId?: string | null },
      reason: 'stub' | 'trim' | 'manual' | 'conversation'
    ) => {
      scopedStore().evictInstance(convId, lookup, reason)
    },
    scopedMessagesForTraceCached: (
      anchorMessageId: string,
      traceId: string,
      agentInstanceId?: string
    ) => {
      const convId = (chat.currentId ?? '').trim()
      if (!convId) return []
      return scopedStore()
        .getRows(convId, { anchorMessageId, traceId, agentInstanceId })
        .slice()
    }
  }
  return { useChatStore: () => chat }
})

vi.mock('../../../../stores/settings', () => ({
  useSettingsStore: () => ({ settings: { rawContentViewEnabled: false } })
}))

vi.mock('../../../../composables/useAgentUi', () => ({
  useAgentsCatalog: () => ref([])
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

function trace(over: Partial<AgentTrace> = {}): AgentTrace {
  return {
    id: 'inst-1',
    name: 'coder',
    role: 'worker',
    status: 'completed',
    depth: 1,
    agentInstanceId: 'inst-1',
    anchorMessageId: 'lead',
    userExpanded: true,
    ...over
  }
}

function scopedRow(id: string, over: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 1,
    anchorMessageId: 'lead',
    traceId: 'inst-1',
    agentInstanceId: 'inst-1',
    ...over
  }
}

const mountedApps: App[] = []

function mountFrame(
  traceValue: AgentTrace,
  provides: Record<string, unknown> = {}
): HTMLElement {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(SubAgentFrame, {
    // Root props are shallow — a deep reactive trace is what lets the frame's own
    // expand/collapse mutation (search hit) re-render, as it does from the store.
    trace: reactive(traceValue),
    anchorMessageId: 'lead',
    messages: [],
    messageUi,
    createdAt: 1,
    generating: false,
    isActiveGenerationMessage: false,
    hostTool: null
  })
  app.use(i18n)
  for (const [key, value] of Object.entries(provides)) app.provide(key, value)
  mountedApps.push(app)
  app.mount(host)
  return host
}

async function flush() {
  for (let i = 0; i < 4; i += 1) await nextTick()
}

/** Content blocks and tool rows in DOM order. */
function frameOrder(host: HTMLElement): string[] {
  return [...host.querySelectorAll('[data-sub-agent-content-id],[data-tool-call-id]')].map(
    el =>
      (el as HTMLElement).dataset.subAgentContentId
      ?? (el as HTMLElement).dataset.toolCallId
      ?? ''
  )
}

beforeEach(() => {
  applyUiLocale('zh-CN')
  resetConversationScopedStoreForTests()
})

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('SubAgentFrame round content', () => {
  it('renders each round text before that round own tools', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('r1', {
        content: '第一轮结论',
        createdAt: 1,
        toolCalls: [{ id: 't1', name: 'file_list', arguments: '{}', status: 'success' }]
      }),
      scopedRow('r2', {
        content: '',
        createdAt: 2,
        toolCalls: [{ id: 't2', name: 'file_read', arguments: '{}', status: 'success' }]
      }),
      scopedRow('r3', { content: '最终 handoff', createdAt: 3 })
    ])
    const host = mountFrame(trace())
    await flush()

    expect(frameOrder(host)).toEqual(['r1', 't1', 't2', 'r3'])
    expect(host.querySelector('[data-sub-agent-content-id="r1"]')?.textContent)
      .toContain('第一轮结论')
    expect(host.querySelector('[data-sub-agent-content-id="r3"]')?.textContent)
      .toContain('最终 handoff')
  })

  it('renders round text as markdown and keeps the host stub hidden', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('stub', {
        role: 'user',
        content: 'Begin. Your assigned task is in the system prompt under **Assigned task**.',
        createdAt: 0
      }),
      scopedRow('r1', { content: '**加粗**结论', createdAt: 1 })
    ])
    const host = mountFrame(trace())
    await flush()

    const block = host.querySelector('[data-sub-agent-content-id="r1"]') as HTMLElement
    expect(block.innerHTML).toContain('<strong>')
    expect(host.textContent).not.toContain('Assigned task')
  })

  it('shows one line of the latest round text while collapsed', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('r1', { content: '第一轮结论', createdAt: 1 }),
      scopedRow('r3', { content: '## 最终 handoff\n更多细节', createdAt: 2 })
    ])
    const host = mountFrame(trace({ userExpanded: false }))
    await flush()

    const preview = host.querySelector('[data-sub-agent-content-preview]')
    expect(preview?.textContent?.trim()).toBe('最终 handoff')
    expect(host.querySelectorAll('[data-sub-agent-content-id]')).toHaveLength(0)
  })

  it('expands a collapsed frame when the search hit is a round content row', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('r1', { content: '第一轮结论', createdAt: 1 }),
      scopedRow('r3', { content: '最终 handoff', createdAt: 2 })
    ])
    const host = mountFrame(trace({ userExpanded: false }), {
      currentConversationSearchContentIds: ref(['r3'])
    })
    await flush()

    expect(host.querySelector('[data-sub-agent-content-id="r3"]')).toBeTruthy()
  })
})

/**
 * A nested spawn is persisted on *its issuer's* scoped row (`agentTrace`), while the
 * issuer's own trace sits on the layer above — so the frame has to seed the tree with
 * itself, then the deeper levels come from its own rows.
 */
function childTrace(over: Partial<AgentTrace> = {}): AgentTrace {
  return {
    id: 'inst-2',
    name: 'explore',
    role: 'worker',
    status: 'completed',
    depth: 2,
    agentInstanceId: 'inst-2',
    parentTraceId: 'inst-1',
    parentToolCallId: 'tc-child',
    anchorMessageId: 'p-round-1',
    userExpanded: false,
    ...over
  }
}

/** Issuer row (this frame's own round) that hosted the nested spawn. */
function parentRoundRow(child: AgentTrace): ChatMessage {
  return scopedRow('p-round-1', {
    content: '父层结论',
    createdAt: 1,
    toolCalls: [{ id: 'tc-child', name: 'run_subagent', arguments: '{}', status: 'success' }],
    agentTrace: [child]
  })
}

/** Child's own scoped row — anchored on the issuer row, owned by the child instance. */
function childRoundRow(over: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: 'c-round-1',
    role: 'assistant',
    content: '子层结论',
    status: 'done',
    createdAt: 2,
    anchorMessageId: 'p-round-1',
    traceId: 'inst-2',
    agentInstanceId: 'inst-2',
    ...over
  }
}

function mountParentWithChild(
  child: AgentTrace,
  extraRows: ChatMessage[] = [],
  provides: Record<string, unknown> = {}
): HTMLElement {
  useConversationScopedStore().ingestRows('c1', [parentRoundRow(child), ...extraRows])
  return mountFrame(trace({ userExpanded: true }), provides)
}

describe('nested child frames', () => {
  it('degrades a terminal collapsed child to the same stub as a top-level frame', async () => {
    const child = childTrace({ summaryLine: '读文件 3 次' })
    const host = mountParentWithChild(child, [childRoundRow()])
    await flush()

    const stubs = host.querySelectorAll('.sub-agent-frame-stub')
    expect(stubs).toHaveLength(1)
    expect(stubs[0]?.textContent).toContain('读文件 3 次')
    // Parent stays a full frame; the child's rounds and content never mount.
    expect(host.querySelector('[data-sub-agent-content-id="p-round-1"]')).toBeTruthy()
    expect(host.querySelector('[data-tool-call-id="tc-child"]')).toBeTruthy()
    expect(host.querySelector('[data-sub-agent-content-id="c-round-1"]')).toBeNull()
  })

  it('keeps a child with a pending approval on the full frame', async () => {
    const child = childTrace({ summaryLine: '等待批准' })
    const approval = {
      id: 'ap1',
      name: 'file_write',
      arguments: '{}',
      status: 'pending_approval' as const
    }
    const host = mountParentWithChild(child, [
      childRoundRow({ toolCalls: [approval] })
    ])
    await flush()

    // Stub has no approval card, so the pending row keeps the child on a full frame.
    expect(host.querySelectorAll('.sub-agent-frame-stub')).toHaveLength(0)
    expect(host.querySelectorAll('.sub-agent-frame')).toHaveLength(2)
    expect(host.querySelector('[data-tool-call-id="ap1"]')).toBeTruthy()
  })

  it('keeps a search-hit child expanded instead of stubbing it away', async () => {
    const child = reactive(childTrace({ summaryLine: '读文件 1 次' }))
    const host = mountParentWithChild(
      child,
      [childRoundRow({ content: '中间结论：先看仓库' })],
      { currentConversationSearchContentIds: ref(['c-round-1']) }
    )
    await flush()

    expect(host.querySelectorAll('.sub-agent-frame-stub')).toHaveLength(0)
    // Parent + child both stay full frames, and the hit is reachable in the child.
    expect(host.querySelectorAll('.sub-agent-frame')).toHaveLength(2)
    expect(host.querySelector('[data-sub-agent-content-id="c-round-1"]')?.textContent)
      .toContain('中间结论')
  })
})
