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

  function appendOutput(tab: WorkspaceConsoleTab, text: string) {
    tab.output = `${tab.output}${text}`.slice(-MAX_PENDING_OUTPUT_CHARS)
  }

  function handleStream(event: StreamEvent) {
    if (event.kind === 'console_output_delta') {
      const tab = tabs.value.find(item => item.id === event.sessionId)
      if (tab) appendOutput(tab, event.output)
      return
    }
    if (event.kind === 'console_session_exited') {
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
    if (tabs.value.some(tab => tab.id === sessionId)) activeSessionId.value = sessionId
  }

  async function write(sessionId: string, data: string) {
    await writeConsoleSession(sessionId, data)
  }

  async function resize(sessionId: string, cols: number, rows: number) {
    await resizeConsoleSession(sessionId, cols, rows)
  }

  async function close(sessionId: string) {
    const tab = tabs.value.find(item => item.id === sessionId)
    if (!tab) return
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
    unlisten?.()
    unlisten = null
  }

  return { tabs, activeSessionId, activeTab, create, select, write, resize, close, closeWorkspace, dispose }
})
