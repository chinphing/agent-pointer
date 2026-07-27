import { isImConversation } from '../../../lib/channel-labels'
import { maybeUpdateConversationTitle } from '../../../lib/conversationTitle'
import { dedupeImInboundUserMessages } from '../../../lib/imMessageDedupe'
import {
  isEphemeralDesktopNoticeMessage,
  isGenerationCancelledMessage
} from '../../../lib/assistantMessageKind'
import { flushStreamDeltaBuffers } from '../../../lib/reasoningDeltaBatch'
import { recordTurnDone } from '../../../lib/turnElapsed'
import { useSkillsStore } from '../../skills'
import { useSettingsStore } from '../../settings'
import type { ChatMessage, StreamEvent } from '../../../types/chat'
import {
  markTrailingAssistantCancelled,
  normalizeInterruptedAssistantStatuses,
  removeAssistantMessage,
  removeTrailingDiscardableEmptyAssistant,
  uid
} from '../helpers'
import { playTaskCompleteSoundIfEnabled } from '../../../lib/taskCompleteSound'
import type { StreamHandlerContext } from './types'

type UiToast = Extract<StreamEvent, { kind: 'ui_toast' }>
type ToolRoundsExhausted = Extract<StreamEvent, { kind: 'tool_rounds_exhausted' }>
type WorkspaceUpdated = Extract<StreamEvent, { kind: 'workspace_updated' }>
type ComputerMonitorPickRequired = Extract<StreamEvent, { kind: 'computer_monitor_pick_required' }>
type ComputerMonitorUpdated = Extract<StreamEvent, { kind: 'computer_monitor_updated' }>
type SkillsUpdated = Extract<StreamEvent, { kind: 'skills_updated' }>
type ImSessionForked = Extract<StreamEvent, { kind: 'im_session_forked' }>
type ImSessionAgentChanged = Extract<StreamEvent, { kind: 'im_session_agent_changed' }>
type UserMessageAttachmentsUpdated = Extract<StreamEvent, { kind: 'user_message_attachments_updated' }>
type InjectedUserMessage = Extract<StreamEvent, { kind: 'injected_user_message' }>
type InjectedAssistantMessage = Extract<StreamEvent, { kind: 'injected_assistant_message' }>
type InjectedAssistantMessageUpdate = Extract<StreamEvent, { kind: 'injected_assistant_message_update' }>
type AssistantRoundScreen = Extract<StreamEvent, { kind: 'assistant_round_screen' }>
type StreamError = Extract<StreamEvent, { kind: 'error' }>
type Done = Extract<StreamEvent, { kind: 'done' }>

export function handleUiToast(ctx: StreamHandlerContext, e: UiToast) {
  if (e.conversationId && e.conversationId !== ctx.currentId.value) return
  const lv = e.level
  const level: 'success' | 'warning' | 'error' =
    lv === 'error' ? 'error' : lv === 'warning' ? 'warning' : 'success'
  ctx.showUiToast(e.message, level)
}

export function handleToolRoundsExhausted(ctx: StreamHandlerContext, e: ToolRoundsExhausted) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const suffix = e.willRetryAfterCompress ? '\n\n（正在压缩较早对话摘要…）' : ''
  conv.messages.push({
    id: uid(),
    role: 'assistant',
    content: `【提示】${e.message}${suffix}`,
    status: 'done',
    createdAt: Date.now(),
    toolCalls: []
  })
  conv.updatedAt = Date.now()
  ctx.markMetaDirty(e.conversationId)
}

export function handleWorkspaceUpdated(ctx: StreamHandlerContext, e: WorkspaceUpdated) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (conv) {
    const shouldPersist = e.isEphemeralSandbox || !isImConversation(e.conversationId)
    if (shouldPersist) {
      conv.workspaceRoot = e.workspaceRoot
      if (e.isEphemeralSandbox) {
        conv.workspaceUserSet = false
        conv.workspaceInheritDisabled = true
      }
      conv.updatedAt = Date.now()
      ctx.markMetaDirty(e.conversationId)
    }
  }
  if (e.isEphemeralSandbox) {
    ctx.showUiToast('已自动创建临时工作目录，可在输入框下方更换为项目目录', 'warning')
  }
}

export function handleComputerMonitorPickRequired(
  ctx: StreamHandlerContext,
  e: ComputerMonitorPickRequired
) {
  ctx.computerMonitorPickRequest.value = {
    conversationId: e.conversationId,
    messageId: e.messageId,
    toolCallId: e.toolCallId,
    monitors: e.monitors
  }
}

export function handleComputerMonitorUpdated(ctx: StreamHandlerContext, e: ComputerMonitorUpdated) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (conv) {
    conv.computerMonitorId = e.monitorId ?? undefined
    conv.updatedAt = Date.now()
    ctx.markMetaDirty(e.conversationId)
  }
}

export function handleSkillsUpdated(_ctx: StreamHandlerContext, _e: SkillsUpdated) {
  // Backend already persisted agentSkillOverrides[lead]; refresh catalog + settings.
  void useSkillsStore().load({ rescan: true })
  void useSettingsStore().load()
}

export function handleImSessionForked(ctx: StreamHandlerContext, e: ImSessionForked) {
  ctx.ensureImConversation(e.conversationId, e.title)
  const forked = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (forked) {
    ctx.applySessionAgentToConversation(forked, e.leadAgentId, e.agentMode)
  }
  ctx.currentId.value = e.conversationId
  ctx.loadActiveComposerDraft(e.conversationId)
  ctx.markMetaDirty(e.conversationId)
}

export function handleImSessionAgentChanged(ctx: StreamHandlerContext, e: ImSessionAgentChanged) {
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (conv) {
    ctx.applySessionAgentToConversation(conv, e.leadAgentId, e.agentMode)
    ctx.markMetaDirty(e.conversationId)
  }
}

export function handleUserMessageAttachmentsUpdated(
  ctx: StreamHandlerContext,
  e: UserMessageAttachmentsUpdated
) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const msg = conv.messages.find(m => m.id === e.messageId)
  if (!msg) return
  msg.attachments = e.attachments
  if (e.content !== undefined) msg.content = e.content
  conv.updatedAt = Date.now()
}

export function handleInjectedUserMessage(ctx: StreamHandlerContext, e: InjectedUserMessage) {
  ctx.ensureImConversation(e.conversationId)
  ctx.ensureCronStreamConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const existing = conv.messages.find(m => m.id === e.messageId)
  if (existing) {
    existing.content = e.content
    if (e.attachments?.length) existing.attachments = e.attachments
  } else {
    conv.messages.push({
      id: e.messageId,
      role: 'user',
      content: e.content,
      status: 'done',
      createdAt: Date.now(),
      ...(e.attachments?.length ? { attachments: e.attachments } : {})
    })
  }
  conv.messages = dedupeImInboundUserMessages(e.conversationId, conv.messages)
  maybeUpdateConversationTitle(conv)
  conv.updatedAt = Date.now()
}

export function handleInjectedAssistantMessage(
  ctx: StreamHandlerContext,
  e: InjectedAssistantMessage
) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const existingRow = conv.messages.find(m => m.id === e.messageId)
  if (
    existingRow &&
    existingRow.role === 'assistant' &&
    isEphemeralDesktopNoticeMessage(existingRow)
  ) {
    existingRow.content = e.content
    conv.updatedAt = Date.now()
    ctx.scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
    return
  }
  if (existingRow) return
  const row: ChatMessage = {
    id: e.messageId,
    role: 'assistant',
    content: e.content,
    status: 'done',
    createdAt: Date.now(),
    toolCalls: []
  }
  const last = conv.messages[conv.messages.length - 1]
  if (last?.role === 'assistant' && last.status === 'streaming') {
    conv.messages.splice(conv.messages.length - 1, 0, row)
  } else {
    conv.messages.push(row)
  }
  conv.updatedAt = Date.now()
  ctx.scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
}

export function handleInjectedAssistantMessageUpdate(
  ctx: StreamHandlerContext,
  e: InjectedAssistantMessageUpdate
) {
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const msg = conv.messages.find(m => m.id === e.messageId)
  if (msg && msg.role === 'assistant' && isEphemeralDesktopNoticeMessage(msg)) {
    msg.content = e.content
    conv.updatedAt = Date.now()
    ctx.scheduleDesktopNoticeRemoval(e.conversationId, e.messageId)
  }
}

export function handleAssistantRoundScreen(ctx: StreamHandlerContext, e: AssistantRoundScreen) {
  const r = ctx.findMessage(e.messageId)
  if (!r || r.conv.id !== e.conversationId) return
  if (r.msg.role !== 'assistant') return
  r.msg.computerRoundScreenRelPath = e.annotatedRelPath
  r.conv.updatedAt = Date.now()
}

export function handleStreamError(ctx: StreamHandlerContext, e: StreamError) {
  flushStreamDeltaBuffers(e.messageId ?? undefined)
  const cancelled = isGenerationCancelledMessage(e.message)
  const eventConvId = e.conversationId?.trim() || ''
  const fallbackConvId = eventConvId || ctx.currentId.value?.trim() || ''
  let affectedId: string | null = null
  if (e.messageId) {
    const r = ctx.findMessage(e.messageId, eventConvId || undefined)
    if (r) {
      if (cancelled) {
        // Keep the row so the muted「已停止生成」caption remains visible.
        r.msg.status = 'cancelled'
        r.msg.errorMessage = '已停止生成'
        r.msg.contentStreaming = false
      } else {
        r.msg.status = 'error'
        r.msg.errorMessage = e.message
        r.msg.contentStreaming = false
      }
      ctx.clearRunState(r.conv.id)
      affectedId = r.conv.id
    } else if (fallbackConvId) {
      if (ctx.isConversationGenerating(fallbackConvId)) {
        ctx.clearRunState(fallbackConvId)
      }
      affectedId = fallbackConvId
    }
  } else if (cancelled) {
    const conv = ctx.conversations.value.find(c => c.id === fallbackConvId)
    if (conv) {
      markTrailingAssistantCancelled(conv)
      ctx.clearRunState(conv.id)
      affectedId = conv.id
    } else {
      affectedId = fallbackConvId || null
    }
  } else {
    const conv = ctx.conversations.value.find(c => c.id === fallbackConvId)
    if (conv) {
      conv.messages.push({
        id: uid(),
        role: 'assistant',
        content: '',
        status: 'error',
        createdAt: Date.now(),
        toolCalls: [],
        errorMessage: e.message
      })
      conv.updatedAt = Date.now()
      affectedId = conv.id
    }
    if (fallbackConvId) ctx.clearRunState(fallbackConvId)
    else ctx.clearAllRunStates()
  }
  if (affectedId) ctx.markMetaDirty(affectedId)
}

export function handleDone(ctx: StreamHandlerContext, e: Done) {
  const convId = e.conversationId?.trim() || ctx.currentId.value?.trim() || ''
  try {
    if (convId) recordTurnDone(convId)
    if (convId) ctx.clearRunState(convId)
    flushStreamDeltaBuffers()
    if (convId) ctx.ensureImConversation(convId)
    const conv = convId ? ctx.conversations.value.find(c => c.id === convId) : undefined
    if (conv) {
      normalizeInterruptedAssistantStatuses([conv])
      removeTrailingDiscardableEmptyAssistant(conv)
      if (e.toolRoundsUsedTotal != null) conv.toolRoundsUsed = e.toolRoundsUsedTotal
      if (e.toolRoundsUsedSupervisorTotal != null) {
        conv.toolRoundsUsedSupervisor = e.toolRoundsUsedSupervisorTotal
      }
      ctx.persistAppend(convId)
      if (convId.startsWith('cron:') || convId.startsWith('webhook:')) {
        ctx.refreshConversationMessages(convId)
      }
    }
    if (convId) ctx.markMetaDirty(convId)
  } finally {
    // Chime must not depend on persist/normalize succeeding.
    playTaskCompleteSoundIfEnabled()
  }
}
