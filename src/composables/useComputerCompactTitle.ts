import { computed, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'
import { activeComputerTrace, delegatedComputerSubTaskId, isComputerToolName } from '../lib/computerExecuting'
import { taskBoardCompactSummary } from '../lib/taskBoardCollapsedLine'
import { visibleToolCalls } from '../lib/messageTooling'
import {
  compactToolCallStatusLine,
  latestToolCallForCompactStatus
} from '../lib/toolCallDisplay'
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

function visibleComputerToolCalls(message: ChatMessage | undefined): ToolCall[] {
  if (!message) return []
  const trace = activeComputerTrace(message)
  const calls = trace?.session?.toolCalls ?? message.toolCalls ?? []
  return visibleToolCalls(calls, COMPUTER_HIDE_TOOL_NAMES, false, true).filter(tc =>
    isComputerToolName(tc.name)
  )
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

  const planSummary = computed(() => {
    const conv = chat.current
    if (!conv) return null
    const msg = activeMessage.value
    const msgId = activeGeneratingMessageId.value
    const subTaskId = delegatedComputerSubTaskId(activeComputerTrace(msg))
    const document =
      chat.compactTaskBoardDocument(conv.id, msgId, subTaskId) ??
      chat.compactTaskBoardDocument(conv.id, null, subTaskId)
    if (!document) return null
    return taskBoardCompactSummary(document)
  })

  const planLine = computed((): string | null => planSummary.value?.fullLine ?? null)

  const statusLine = computed((): string => {
    if (stoppedHint.value) return '已停止'
    if (computerMonitorPickRequest.value) return '请选择操控屏幕…'

    const msg = activeMessage.value
    const tool = latestToolCallForCompactStatus(visibleComputerToolCalls(msg))
    if (tool) return compactToolCallStatusLine(tool)

    const body = streamBodyFromMessage(msg)
    const preview = body.toolNamePreview?.trim()
    if (preview && isComputerToolName(preview)) return '执行中…'

    const chars = streamedCharCountFromBody(body)
    return thinkingLabel(chars)
  })

  const twoLines = computed(() => planSummary.value != null)

  return { planSummary, planLine, statusLine, twoLines }
}
