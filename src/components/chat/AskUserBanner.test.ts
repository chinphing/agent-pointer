// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatMessage } from '../../types/chat'
import { i18n } from '../../i18n'
import { applyUiLocale } from '../../lib/uiLocale'
import {
  resetConversationScopedStoreForTests,
  useConversationScopedStore
} from '../../lib/conversationScoped'
import { submitAskUser } from '../../lib/api'
import { pendingAskUserToolCalls } from '../../lib/askUserBanner'
import AskUserBanner from './AskUserBanner.vue'

const hoisted = vi.hoisted(() => ({
  chatState: {
    currentId: 'c1' as string | null,
    current: null as { id: string; messages: ChatMessage[] } | null
  }
}))

vi.mock('../../stores/chat', () => ({
  useChatStore: () => hoisted.chatState
}))

vi.mock('../../lib/api', () => ({
  submitAskUser: vi.fn(async () => {})
}))

// Count the banner's queue scans: the regression was `pending` subscribing to the
// conversation-wide live signal, so every streamed chunk rescanned all rows.
vi.mock('../../lib/askUserBanner', async importOriginal => {
  const actual = await importOriginal<typeof import('../../lib/askUserBanner')>()
  return { ...actual, pendingAskUserToolCalls: vi.fn(actual.pendingAskUserToolCalls) }
})

const pendingScan = vi.mocked(pendingAskUserToolCalls)

const mountedApps: Array<ReturnType<typeof createApp>> = []

function deepScopedRow(): ChatMessage {
  return {
    id: 'scoped-deep',
    role: 'assistant',
    content: '',
    status: 'streaming',
    createdAt: 1,
    anchorMessageId: 'lead-1',
    agentInstanceId: 'inst-deep',
    spawnDepth: 2,
    toolCalls: [
      {
        id: 'tc-deep',
        name: 'ask_user',
        status: 'pending',
        arguments: JSON.stringify({
          question: '深层提问：选哪个？',
          options: [{ label: '选项A' }, { label: '选项B' }],
          multi_select: false
        })
      }
    ]
  }
}

function mountBanner() {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(AskUserBanner)
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
  hoisted.chatState.currentId = 'c1'
  hoisted.chatState.current = { id: 'c1', messages: [] }
  vi.mocked(submitAskUser).mockClear()
  pendingScan.mockClear()
})

afterEach(() => {
  vi.useRealTimers()
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('AskUserBanner', () => {
  it('shows a deep scoped pending ask_user without any frame being mounted', async () => {
    useConversationScopedStore().ingestRows('c1', [deepScopedRow()])
    const host = mountBanner()
    await flush()

    const banner = host.querySelector('[data-ask-user-banner]')
    expect(banner).toBeTruthy()
    expect(host.textContent).toContain('深层提问：选哪个？')
    expect(host.textContent).toContain('选项A')
  })

  it('stays hidden when nothing is pending', async () => {
    const host = mountBanner()
    await flush()
    expect(host.querySelector('[data-ask-user-banner]')).toBeNull()
  })

  it('submits the answer and hides 2s later', async () => {
    vi.useFakeTimers()
    const store = useConversationScopedStore()
    store.ingestRows('c1', [deepScopedRow()])
    const host = mountBanner()
    await flush()

    const option = [...host.querySelectorAll('button')]
      .find(btn => btn.textContent?.includes('选项A')) as HTMLButtonElement
    expect(option).toBeTruthy()
    option.click()
    await flush()

    expect(vi.mocked(submitAskUser)).toHaveBeenCalledWith('tc-deep', ['选项A'])
    expect(host.querySelector('[data-ask-user-banner-linger]')).toBeTruthy()

    // The backend flips the tool call to success while the confirmation lingers.
    const row = store.findRow('c1', 'scoped-deep')!
    row.toolCalls![0]!.status = 'success'
    row.toolCalls![0]!.result = JSON.stringify({ selected: ['选项A'] })
    store.touchRow('c1', 'scoped-deep')

    await vi.advanceTimersByTimeAsync(1999)
    await flush()
    expect(host.querySelector('[data-ask-user-banner-linger]')).toBeTruthy()

    await vi.advanceTimersByTimeAsync(1)
    await flush()
    expect(host.querySelector('[data-ask-user-banner]')).toBeNull()
  })

  it('queues a second pending question after the first is answered', async () => {
    vi.useFakeTimers()
    const store = useConversationScopedStore()
    const first = deepScopedRow()
    const second: ChatMessage = {
      ...deepScopedRow(),
      id: 'scoped-deep-2',
      agentInstanceId: 'inst-deep-2',
      toolCalls: [
        {
          id: 'tc-deep-2',
          name: 'ask_user',
          status: 'pending',
          arguments: JSON.stringify({
            question: '第二个问题',
            options: [{ label: 'X' }, { label: 'Y' }],
            multi_select: false
          })
        }
      ]
    }
    store.ingestRows('c1', [first, second])
    const host = mountBanner()
    await flush()

    expect(host.querySelector('[data-ask-user-banner-remaining]')?.textContent)
      .toContain('还有 1 条待回答')

    const option = [...host.querySelectorAll('button')]
      .find(btn => btn.textContent?.includes('选项A')) as HTMLButtonElement
    option.click()
    await flush()

    const row = store.findRow('c1', 'scoped-deep')!
    row.toolCalls![0]!.status = 'success'
    store.touchRow('c1', 'scoped-deep')

    await vi.advanceTimersByTimeAsync(2000)
    await flush()
    expect(host.textContent).toContain('第二个问题')
  })

  it('does not rescan for streamed row text', async () => {
    const store = useConversationScopedStore()
    const row = deepScopedRow()
    store.ingestRows('c1', [row])
    const host = mountBanner()
    await flush()
    expect(host.querySelector('[data-ask-user-banner]')).toBeTruthy()

    const scansBefore = pendingScan.mock.calls.length
    const liveBefore = store.getLiveSignal('c1', 'inst-deep')

    row.content += '流式正文'
    row.thoughts = '还在推理'
    row.toolCalls![0]!.result = 'x'.repeat(200)
    store.touchRow('c1', 'scoped-deep')
    await flush()

    // The live signal really moved — but the banner did not rescan its queue.
    expect(store.getLiveSignal('c1', 'inst-deep')).not.toBe(liveBefore)
    expect(pendingScan.mock.calls.length).toBe(scansBefore)
    expect(host.querySelector('[data-ask-user-banner]')).toBeTruthy()
  })

  it('rescans while an ask_user call still streams its arguments', async () => {
    const store = useConversationScopedStore()
    const row = deepScopedRow()
    store.ingestRows('c1', [row])
    mountBanner()
    await flush()

    const scansBefore = pendingScan.mock.calls.length
    row.toolCalls![0]!.arguments += ',"multi_select":false}'
    store.touchRow('c1', 'scoped-deep')
    await flush()

    // The banner renders the question / options straight from `arguments`.
    expect(pendingScan.mock.calls.length).toBeGreaterThan(scansBefore)
  })

  it('rescans when a scoped ask_user tool call changes status', async () => {
    const store = useConversationScopedStore()
    const row = deepScopedRow()
    store.ingestRows('c1', [row])
    const host = mountBanner()
    await flush()
    expect(host.querySelector('[data-ask-user-banner]')).toBeTruthy()

    const scansBefore = pendingScan.mock.calls.length
    row.toolCalls![0]!.status = 'success'
    store.touchRow('c1', 'scoped-deep')
    await flush()

    expect(pendingScan.mock.calls.length).toBeGreaterThan(scansBefore)
    expect(host.querySelector('[data-ask-user-banner]')).toBeNull()
  })
})
