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
})
