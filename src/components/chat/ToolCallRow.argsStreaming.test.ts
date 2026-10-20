// @vitest-environment happy-dom

import { createApp, nextTick, reactive } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ToolCall } from '../../types/chat'
import { i18n } from '../../i18n'
import { applyUiLocale } from '../../lib/uiLocale'
import { resetToolCallArgumentsCache } from '../../lib/toolCallArguments'
import ToolCallRow from './ToolCallRow.vue'

const hoisted = vi.hoisted(() => ({
  chatState: {
    currentId: 'c1' as string | null,
    current: { id: 'c1', workspaceRoot: '/repo' } as { id: string; workspaceRoot?: string } | null,
    terminalLiveViewReadyToolCallId: '' as string,
    ensureToolCallBody: vi.fn(),
    releaseToolCallBody: vi.fn(),
    approve: vi.fn(),
    abortTerminalOnly: vi.fn(),
    cancelBackgroundJob: vi.fn(async () => {}),
    endWaitKeepBackground: vi.fn(async () => {}),
    openTerminalLivePopup: vi.fn()
  }
}))

vi.mock('../../stores/chat', () => ({
  useChatStore: () => hoisted.chatState
}))

const mountedApps: Array<ReturnType<typeof createApp>> = []

/** Reactive props, so a streamed `arguments` update re-renders the row like the store does. */
function mountRow(toolCall: ToolCall) {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(ToolCallRow, {
    toolCall: reactive(toolCall),
    showToolCallResults: false
  })
  app.use(i18n)
  mountedApps.push(app)
  app.mount(host)
  return host
}

async function flush() {
  for (let i = 0; i < 4; i += 1) await nextTick()
}

function header(host: HTMLElement): HTMLElement {
  return host.querySelector('.tool-call-trigger') as HTMLElement
}

/** Every `JSON.parse` input seen during a mount — the render-perf HUD's blind spot. */
function parsedInputs(spy: { mock: { calls: unknown[][] } }): string[] {
  return spy.mock.calls
    .map(args => args[0])
    .filter((input): input is string => typeof input === 'string')
}

beforeEach(() => {
  applyUiLocale('zh-CN')
  resetToolCallArgumentsCache()
  hoisted.chatState.ensureToolCallBody.mockClear()
  hoisted.chatState.releaseToolCallBody.mockClear()
})

afterEach(() => {
  vi.restoreAllMocks()
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('ToolCallRow while arguments stream', () => {
  it('shows the backend displaySummary without parsing the arguments', async () => {
    const streaming: ToolCall = {
      id: 'tc-stream',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      displaySummary: 'src/lib/big.ts',
      arguments: '{"path":"/repo/src/lib/big.ts","content":"export const a = 1\\nexport const b'
    }

    const parseSpy = vi.spyOn(JSON, 'parse')
    const host = mountRow(streaming)
    await flush()

    expect(header(host).textContent).toContain('src/lib/big.ts')
    expect(parsedInputs(parseSpy).filter(text => text.includes('/repo/src/lib/big.ts'))).toEqual([])
  })

  it('shows the raw half written args instead of a parse error when expanded', async () => {
    const raw = '{"path":"/repo/src/lib/big.ts","content":"export const a = 1\\nexport const b'
    const streaming: ToolCall = {
      id: 'tc-stream-2',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      displaySummary: 'src/lib/big.ts',
      arguments: raw
    }

    const parseSpy = vi.spyOn(JSON, 'parse')
    const host = mountRow(streaming)
    await flush()
    header(host).click()
    await flush()

    const pre = host.querySelector('pre')
    expect(pre?.textContent).toBe(raw)
    expect(host.textContent).not.toContain('参数解析失败')
    expect(hoisted.chatState.ensureToolCallBody).toHaveBeenCalledWith('tc-stream-2')
    expect(parsedInputs(parseSpy).filter(text => text.includes('/repo/src/lib/big.ts'))).toEqual([])
  })

  it('fills the path in as soon as the payload closes', async () => {
    const live = reactive<ToolCall>({
      id: 'tc-close',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      arguments: '{"path":"/repo/src/lib/big.ts","content":"export const a = 1'
    })
    const host = mountRow(live)
    await flush()
    expect(host.textContent).not.toContain('src/lib/big.ts')

    live.arguments = JSON.stringify({ path: '/repo/src/lib/big.ts', content: 'a\n' })
    await flush()

    // The row is bound to a live store object, so the path chip appears on the
    // next render — that is the "闭合瞬间出现" behavior.
    expect(host.textContent).toContain('src/lib/big.ts')
  })
})
