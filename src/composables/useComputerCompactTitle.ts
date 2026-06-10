import { computed, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'
import { activeComputerTrace, isComputerToolName } from '../lib/computerExecuting'
import { taskBoardCollapsedLine } from '../lib/taskBoardCollapsedLine'
import { visibleToolCalls } from '../lib/messageTooling'
import { thinkingLabel, streamedCharCountFromBody } from '../lib/thinkingIndicator'
import type { ChatMessage, ToolCall } from '../types/chat'

const COMPUTER_HIDE_TOOL_NAMES = [
  'task_board_init',
  'task_board_patch',
  'task_board_replace',
  'task_board_finalize',
  'task_board_sync_finding',
  'task_board_check_deps',
  'task_board_prune',
  'action_verify'
]

function toolInProgress(tc: ToolCall): boolean {
  return tc.status === 'running' || tc.status === 'pending' || tc.status === 'pending_approval'
}

function formatToolLine(tc: ToolCall): string {
  const label = tc.displayLabel?.trim() || tc.name
  const summary = tc.displaySummary?.trim()
  return summary ? `${label} · ${summary}` : label
}

function visibleComputerToolCalls(message: ChatMessage | undefined): ToolCall[] {
  if (!message) return []
  const trace = activeComputerTrace(message)
  const calls = trace?.session?.toolCalls ?? message.toolCalls ?? []
  return visibleToolCalls(calls, COMPUTER_HIDE_TOOL_NAMES, false, true).filter(tc =>
    isComputerToolName(tc.name)
  )
}

function activeComputerTool(calls: ToolCall[]): ToolCall | undefined {
  for (let i = calls.length - 1; i >= 0; i -= 1) {
    if (toolInProgress(calls[i])) return calls[i]
  }
  return undefined
}

function streamBodyFromMessage(message: ChatMessage | undefined) {
  if (!message) return {}
  const trace = activeComputerTrace(message)
  if (trace?.session) {
    return {
      content: trace.session.responseTextDraft ?? trace.content,
      rawContent: trace.session.rawContent,
      thoughts: trace.session.thoughts,
      toolNamePreview: trace.session.toolNamePreview,
      responseTextDraft: trace.session.responseTextDraft,
      reasoning: trace.session.reasoning,
      toolCalls: trace.session.toolCalls
    }
  }
  return {
    content: message.content,
    rawContent: message.rawContent,
    thoughts: message.thoughts,
    toolNamePreview: message.toolNamePreview,
    responseTextDraft: message.responseTextDraft,
    reasoning: message.reasoning,
    toolCalls: message.toolCalls
  }
}

export function useComputerCompactTitle(stoppedHint: Ref<boolean>) {
  const chat = useChatStore()
  const { activeGeneratingMessageId, computerMonitorPickRequest } = storeToRefs(chat)

  const activeMessage = computed(() => {
    const conv = chat.current
    const id = activeGeneratingMessageId.value
    if (!conv || !id) return undefined
    return conv.messages.find(m => m.id === id)
  })

  const planLine = computed((): string | null => {
    const conv = chat.current
    const msgId = activeGeneratingMessageId.value
    if (!conv || !msgId) return null
    const boards = chat.parentBoardsBoundToMessage(conv.id, msgId)
    const active = boards.find(b => b.isActive) ?? boards[0]
    if (!active?.document) return null
    return taskBoardCollapsedLine(active.document)
  })

  const statusLine = computed((): string => {
    if (stoppedHint.value) return '已停止'
    if (computerMonitorPickRequest.value) return '请选择操控屏幕…'

    const msg = activeMessage.value
    const tool = activeComputerTool(visibleComputerToolCalls(msg))
    if (tool) return formatToolLine(tool)

    const body = streamBodyFromMessage(msg)
    const preview = body.toolNamePreview?.trim()
    if (preview && isComputerToolName(preview)) return '执行中…'

    const chars = streamedCharCountFromBody(body)
    return thinkingLabel(chars)
  })

  const twoLines = computed(() => planLine.value != null && planLine.value.length > 0)

  return { planLine, statusLine, twoLines }
}
