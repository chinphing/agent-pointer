import { computed, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'
import { activeComputerTrace, delegatedComputerSubTaskId, isComputerToolName } from '../lib/computerExecuting'
import { latestSubAgentBodyModelFromScoped } from '../lib/subAgentMessages'
import { useConversationScopedStore } from '../lib/conversationScoped'
import { taskBoardCompactSummary } from '../lib/taskBoardCollapsedLine'
import { visibleToolCalls } from '../lib/messageTooling'
import {
  compactToolCallStatusLine,
  latestToolCallForCompactStatus
} from '../lib/toolCallDisplay'
import { thinkingLabel, thinkingCharCountForCurrentRound } from '../lib/thinkingIndicator'
import { isPlannerPhaseThoughts, PLANNER_PHASE_THOUGHTS } from '../lib/plannerPhase'
import type { ChatMessage, ToolCall } from '../types/chat'

const COMPUTER_HIDE_TOOL_NAMES = [
  'task_board_init',
  'task_board_patch',
  'task_board_replace',
  'task_board_finalize',
  'task_board_check_deps',
  'task_board_prune',
]

function planningToolCalls(message: ChatMessage | undefined): ToolCall[] {
  if (!message?.toolCalls?.length) return []
  return visibleToolCalls(message.toolCalls, COMPUTER_HIDE_TOOL_NAMES, false, true)
}

function toolCallInProgress(status: ToolCall['status']): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

function visibleComputerToolCalls(
  conv: ReturnType<typeof useChatStore>['current'],
  message: ChatMessage | undefined
): ToolCall[] {
  if (!message) return []
  const trace = activeComputerTrace(message)
  if (trace && conv) {
    const anchor = trace.anchorMessageId?.trim() || message.id
    const scoped = latestSubAgentBodyModelFromScoped(
      useConversationScopedStore().getRows(conv.id, {
        anchorMessageId: anchor,
        traceId: trace.id,
        agentInstanceId: trace.agentInstanceId
      }),
      anchor,
      trace.id,
      trace.status,
      trace.agentInstanceId
    )
    if (scoped?.toolCalls?.length) {
      return visibleToolCalls(scoped.toolCalls, COMPUTER_HIDE_TOOL_NAMES, false, true).filter(tc =>
        isComputerToolName(tc.name)
      )
    }
  }
  const calls = trace?.session?.toolCalls ?? message.toolCalls ?? []
  return visibleToolCalls(calls, COMPUTER_HIDE_TOOL_NAMES, false, true).filter(tc =>
    isComputerToolName(tc.name)
  )
}

function streamBodyFromMessage(
  conv: ReturnType<typeof useChatStore>['current'],
  message: ChatMessage | undefined
) {
  if (!message) return {}
  const trace = activeComputerTrace(message)
  if (trace && conv) {
    const anchor = trace.anchorMessageId?.trim() || message.id
    const scoped = latestSubAgentBodyModelFromScoped(
      useConversationScopedStore().getRows(conv.id, {
        anchorMessageId: anchor,
        traceId: trace.id,
        agentInstanceId: trace.agentInstanceId
      }),
      anchor,
      trace.id,
      trace.status,
      trace.agentInstanceId
    )
    if (scoped) {
      return {
        content: scoped.content,
        rawContent: scoped.rawContent,
        thoughts: scoped.thoughts,
        toolNamePreview: scoped.toolNamePreview,
        responseTextDraft: scoped.responseTextDraft,
        reasoning: scoped.reasoning,
        toolCalls: scoped.toolCalls,
        contentStreaming: scoped.contentStreaming === true
      }
    }
  }
  if (trace?.session) {
    return {
      content: trace.session.responseTextDraft ?? trace.content,
      rawContent: trace.session.rawContent,
      thoughts: trace.session.thoughts,
      toolNamePreview: trace.session.toolNamePreview,
      responseTextDraft: trace.session.responseTextDraft,
      reasoning: trace.session.reasoning,
      toolCalls: trace.session.toolCalls,
      contentStreaming: trace.session.contentStreaming === true
    }
  }
  return {
    content: message.content,
    rawContent: message.rawContent,
    thoughts: message.thoughts,
    toolNamePreview: message.toolNamePreview,
    responseTextDraft: message.responseTextDraft,
    reasoning: message.reasoning,
    toolCalls: message.toolCalls,
    contentStreaming: message.contentStreaming === true
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
    const trace = activeComputerTrace(msg)
    const subTaskId = delegatedComputerSubTaskId(trace)
    const traceId = trace?.id
    const document =
      chat.compactTaskBoardDocument(conv.id, msgId, subTaskId, traceId) ??
      chat.compactTaskBoardDocument(conv.id, null, subTaskId, traceId)
    if (!document) return null
    return taskBoardCompactSummary(document)
  })

  const planLine = computed((): string | null => planSummary.value?.fullLine ?? null)

  const statusLine = computed((): string => {
    if (stoppedHint.value) return '已停止'
    if (computerMonitorPickRequest.value) return '请选择操控屏幕…'

    const msg = activeMessage.value
    const conv = chat.current
    const body = streamBodyFromMessage(conv, msg)

    const computerTool = latestToolCallForCompactStatus(visibleComputerToolCalls(conv, msg))
    if (computerTool) return compactToolCallStatusLine(computerTool, conv?.workspaceRoot)

    const plannerTool = latestToolCallForCompactStatus(planningToolCalls(msg))
    if (plannerTool && toolCallInProgress(plannerTool.status)) {
      return compactToolCallStatusLine(plannerTool, conv?.workspaceRoot)
    }

    if (isPlannerPhaseThoughts(body.thoughts)) {
      if (plannerTool) return compactToolCallStatusLine(plannerTool, conv?.workspaceRoot)
      return PLANNER_PHASE_THOUGHTS
    }

    if (plannerTool) return compactToolCallStatusLine(plannerTool, conv?.workspaceRoot)

    if (planSummary.value) return '准备执行…'

    const preview = body.toolNamePreview?.trim()
    if (preview && isComputerToolName(preview)) return '执行中…'

    const chars = thinkingCharCountForCurrentRound(body)
    return thinkingLabel(chars)
  })

  const twoLines = computed(() => planSummary.value != null)

  return { planSummary, planLine, statusLine, twoLines }
}
