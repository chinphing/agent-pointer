import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  closeConsoleSession,
  createConsoleSession,
  onStream,
  resizeConsoleSession,
  writeConsoleSession,
  type ConsoleSessionInfo
} from '../lib/api'
import type { StreamEvent } from '../types/chat'

const MAX_PENDING_OUTPUT_CHARS = 256_000

export type WorkspaceConsoleTab = ConsoleSessionInfo & {
  output: string
  exited: boolean
  error: string
}

export const useConsoleStore = defineStore('workspaceConsole', () => {
  const tabs = ref<WorkspaceConsoleTab[]>([])
  const activeSessionId = ref('')
  const activeTab = computed(() => tabs.value.find(tab => tab.id === activeSessionId.value) ?? null)
  let unlisten: (() => void) | null = null
  /** Per-session write chain so keystrokes stay in order (invoke/HTTP races). */
  const writeChains = new Map<string, Promise<void>>()
  /** Coalesce PTY floods (vim redraw) into one Vue update per animation frame. */
  const pendingOutput = new Map<string, string>()
  let outputFlushScheduled = false
  let outputFlushHandle: number | null = null

  function trimOutput(buffer: string): string {
    if (buffer.length <= MAX_PENDING_OUTPUT_CHARS) return buffer
    // Prefer cutting on a newline so mid-CSI / alternate-screen frames survive more often.
    const sliced = buffer.slice(-MAX_PENDING_OUTPUT_CHARS)
    const nl = sliced.indexOf('\n')
    return nl >= 0 && nl < 1024 ? sliced.slice(nl + 1) : sliced
  }

  function flushPendingOutput() {
    outputFlushScheduled = false
    outputFlushHandle = null
    if (pendingOutput.size === 0) return
    const batch = Array.from(pendingOutput.entries())
    pendingOutput.clear()
    for (const [sessionId, text] of batch) {
      if (!text) continue
      const tab = tabs.value.find(item => item.id === sessionId)
      if (!tab) continue
      tab.output = trimOutput(`${tab.output}${text}`)
    }
  }

  function scheduleOutputFlush() {
    if (outputFlushScheduled) return
    outputFlushScheduled = true
    const raf =
      typeof requestAnimationFrame === 'function'
        ? requestAnimationFrame
        : (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16) as unknown as number
    outputFlushHandle = raf(() => {
      flushPendingOutput()
    })
  }

  function appendOutput(sessionId: string, text: string) {
    if (!text) return
    pendingOutput.set(sessionId, `${pendingOutput.get(sessionId) ?? ''}${text}`)
    scheduleOutputFlush()
  }

  function handleStream(event: StreamEvent) {
    if (event.kind === 'console_output_delta') {
      appendOutput(event.sessionId, event.output)
      return
    }
    if (event.kind === 'console_session_exited') {
      // Flush any buffered bytes before marking exited so the final frame shows.
      flushPendingOutput()
      const tab = tabs.value.find(item => item.id === event.sessionId)
      if (tab) tab.exited = true
    }
  }

  async function ensureStream() {
    if (!unlisten) unlisten = await onStream(handleStream, 'global')
  }

  async function create(workspaceRoot: string, conversationId: string, cols: number, rows: number, cwd?: string) {
    await ensureStream()
    const info = await createConsoleSession({ workspaceRoot, conversationId, cwd, cols, rows })
    const tab: WorkspaceConsoleTab = { ...info, output: '', exited: false, error: '' }
    tabs.value.push(tab)
    activeSessionId.value = tab.id
    return tab
  }

  function select(sessionId: string) {
    flushPendingOutput()
    if (tabs.value.some(tab => tab.id === sessionId)) activeSessionId.value = sessionId
  }

  async function write(sessionId: string, data: string) {
    const prev = writeChains.get(sessionId) ?? Promise.resolve()
    const next = prev
      .catch(() => {
        /* keep chain alive after a prior write failure */
      })
      .then(async () => {
        await writeConsoleSession(sessionId, data)
      })
    writeChains.set(sessionId, next)
    try {
      await next
    } finally {
      if (writeChains.get(sessionId) === next) writeChains.delete(sessionId)
    }
  }

  async function resize(sessionId: string, cols: number, rows: number) {
    await resizeConsoleSession(sessionId, cols, rows)
  }

  async function close(sessionId: string) {
    const tab = tabs.value.find(item => item.id === sessionId)
    if (!tab) return
    pendingOutput.delete(sessionId)
    writeChains.delete(sessionId)
    await closeConsoleSession(sessionId)
    const index = tabs.value.findIndex(item => item.id === sessionId)
    tabs.value = tabs.value.filter(item => item.id !== sessionId)
    if (activeSessionId.value === sessionId) {
      activeSessionId.value = tabs.value[index]?.id ?? tabs.value[index - 1]?.id ?? ''
    }
  }

  async function closeWorkspace(workspaceRoot: string) {
    const ids = tabs.value
      .filter(tab => tab.workspaceRoot === workspaceRoot)
      .map(tab => tab.id)
    for (const id of ids) await close(id)
  }

  function dispose() {
    if (outputFlushHandle != null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(outputFlushHandle)
    }
    outputFlushHandle = null
    outputFlushScheduled = false
    pendingOutput.clear()
    writeChains.clear()
    unlisten?.()
    unlisten = null
  }

  return { tabs, activeSessionId, activeTab, create, select, write, resize, close, closeWorkspace, dispose }
})
