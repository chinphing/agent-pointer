// @vitest-environment happy-dom

import { createApp, nextTick, reactive, ref, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentTrace, ChatMessage, ToolCall } from '../../../../types/chat'
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
  provides: Record<string, unknown> = {},
  hostTool: ToolCall | null = null
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
    hostTool
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

  it('renders no content text while collapsed', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('r1', { content: '第一轮结论', createdAt: 1 }),
      scopedRow('r3', { content: '## 最终 handoff\n更多细节', createdAt: 2 })
    ])
    const host = mountFrame(trace({ userExpanded: false }))
    await flush()

    // Collapsed frames behave like collapsed tool groups: stats header only, no content preview.
    expect(host.querySelector('[data-sub-agent-content-preview]')).toBeNull()
    expect(host.querySelectorAll('[data-sub-agent-content-id]')).toHaveLength(0)
    expect(host.textContent).not.toContain('最终 handoff')
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

describe('collapsed process row label', () => {
  it('keeps the「过程」placeholder for a finished ask_user-only spawn', async () => {
    // The retry case: the earlier attempt's only tool was ask_user, which is outside the
    // stats whitelist, so a hosted terminal stub has no stats and no orphan title → the
    // host falls back to the placeholder. This is where a bare「过程」next to another
    // spawn's「思考中..」comes from.
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('ask-round', {
        createdAt: 1,
        toolCalls: [
          {
            id: 'tc-ask',
            name: 'ask_user',
            status: 'success',
            arguments: '{"question":"选哪个？","options":[{"label":"A"},{"label":"B"}]}'
          }
        ]
      })
    ])
    vi.useFakeTimers()
    const hostRow: ToolCall = {
      id: 'host-1',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    }
    const host = mountFrame(trace({ status: 'cancelled', userExpanded: false }), {}, hostRow)
    await vi.advanceTimersByTimeAsync(90_000) // past TERMINAL_COLLAPSED_STUB_IDLE_MS (60s)
    await flush()
    expect(host.textContent).toContain('其他 1 次')
    expect(host.textContent).not.toContain('过程')
    vi.useRealTimers()
  })

  it('still falls back to「过程」for a spawn that ran no tool at all', async () => {
    vi.useFakeTimers()
    const hostRow: ToolCall = {
      id: 'host-2',
      name: 'run_subagent',
      status: 'success',
      arguments: '{}'
    }
    const host = mountFrame(trace({ status: 'cancelled', userExpanded: false }), {}, hostRow)
    await vi.advanceTimersByTimeAsync(90_000) // past TERMINAL_COLLAPSED_STUB_IDLE_MS (60s)
    await flush()
    expect(host.textContent).toContain('过程')
    vi.useRealTimers()
  })

  it('drops「过程」while a running spawn already shows its live line', async () => {
    useConversationScopedStore().ingestRows('c1', [
      scopedRow('live', { contentStreaming: true, reasoning: '想一下要不要问用户', createdAt: 1 })
    ])
    const host = mountFrame(trace({ status: 'running', userExpanded: false }))
    await flush()
    expect(host.textContent).toContain('思考中')
    expect(host.textContent).not.toContain('过程')
  })
})

describe('nested child frames', () => {
  it('degrades a terminal collapsed child to the same stub as a top-level frame', async () => {
    const child = childTrace({ summaryLine: '读文件 3 次' })
    const host = mountParentWithChild(child, [childRoundRow()])
    await flush()

    const stubs = host.querySelectorAll('.sub-agent-frame-stub')
    expect(stubs).toHaveLength(1)
    expect(stubs[0]?.textContent).toContain('读文件 3 次')
    // Collapsed stub keeps the stats header only — no content preview, no child text.
    expect(stubs[0]?.querySelector('[data-sub-agent-content-preview]')).toBeNull()
    expect(stubs[0]?.textContent).not.toContain('子层结论')
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

  it('真实数据形状：父帧的 3 个 run_subagent 行各自挂住自己的 depth-2 子帧（P3）', async () => {
    // 夹具取自会话 c7b8306b（差旅费报销第 3 批测试）position 43756-43770 的 payload：
    // 父行自己带 3 个 run_subagent 调用，3 条 depth-2 档案挂在它的 agentTrace 上
    // （宿主行 = 那 3 个调用 id），子行锚在父行上、position 与父行交错。
    const parentInstance = '8ea61fb4-71d6-4cc3-b3c6-ee3f15c921ec'
    const parentRowId = 'agent_msg_49fe2b5d374f4f8e8c7ad4eeb62a4801'
    const calls: ToolCall[] = [
      { id: 'call_00_1wgTxRryGTHv8xilTSLB0605', name: 'run_subagent', status: 'success', arguments: '{}' },
      { id: 'call_01_0f5TYB8pVii5kmuKz93f4234', name: 'run_subagent', status: 'success', arguments: '{}' },
      { id: 'call_02_0xhcyQuJzaUxQELjhqBk2086', name: 'run_subagent', status: 'success', arguments: '{}' }
    ]
    const childIds = [
      '04a1d107-81c2-4f31-9da4-d01c780bafc4',
      '95f07c33-ad3d-41d1-8deb-fa93f9ca318a',
      '773b5e29-ecb1-43e2-8846-408bbc81bf72'
    ]
    const children = childIds.map((id, index) =>
      childTrace({
        id,
        name: '代码探索',
        agentInstanceId: id,
        parentTraceId: parentInstance,
        parentToolCallId: calls[index].id,
        anchorMessageId: parentRowId
      })
    )

    useConversationScopedStore().ingestRows('c1', [
      scopedRow(parentRowId, {
        agentInstanceId: parentInstance,
        traceId: 'sub_task_1e857c70a88f475e91dfac461aecbc3d:coder',
        anchorMessageId: 'lead',
        toolCalls: calls,
        agentTrace: children
      }),
      ...children.map((child, index) =>
        scopedRow(`child-row-${index}`, {
          agentInstanceId: child.id,
          traceId: `${calls[index].id}:${child.id}:explore`,
          anchorMessageId: parentRowId,
          content: `子层结论 ${index}`
        })
      )
    ])

    const host = mountFrame(
      trace({
        id: parentInstance,
        name: 'coder',
        agentInstanceId: parentInstance,
        anchorMessageId: 'lead',
        userExpanded: true
      })
    )
    await flush()

    // 每个子帧都紧跟在**自己的**工单行后面
    for (const call of calls) {
      const row = host.querySelector(`[data-tool-call-id="${call.id}"]`) as HTMLElement | null
      expect(row).toBeTruthy()
      expect(row?.nextElementSibling?.className ?? '').toContain('sub-agent-frame')
    }
    // 父帧 + 3 个子帧；兜底块没有再多画一份（重复渲染会变成 7）
    expect(host.querySelectorAll('.sub-agent-frame, .sub-agent-frame-stub')).toHaveLength(4)
  })

  it('paints the nested child under its own run_subagent row (P3)', async () => {
    const child = childTrace({ status: 'running', summaryLine: '' })
    const host = mountParentWithChild(child, [childRoundRow()])
    await flush()

    const row = host.querySelector('[data-tool-call-id="tc-child"]') as HTMLElement | null
    expect(row).toBeTruthy()
    // 父帧 + 子帧都画出来了
    expect(host.querySelectorAll('.sub-agent-frame')).toHaveLength(2)
    // 子帧紧跟在它的宿主工单行后面（而不是帧体末尾的平铺块）
    expect(row?.nextElementSibling?.classList.contains('sub-agent-frame')).toBe(true)
  })
})
