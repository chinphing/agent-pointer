import { t } from '../../../i18n'
import { isImConversation } from '../../../lib/channel-labels'
import { maybeUpdateConversationTitle } from '../../../lib/conversationTitle'
import { dedupeImInboundUserMessages } from '../../../lib/imMessageDedupe'
import {
  isEphemeralDesktopNoticeMessage,
  isGenerationCancelledMessage
} from '../../../lib/assistantMessageKind'
import { flushStreamDeltaBuffers } from '../../../lib/reasoningDeltaBatch'
import { notifyChannelPairingPending } from '../../../lib/channelPairingPendingBus'
import {
  disarmTaskCompleteAudio,
  playTaskCompleteSoundIfEnabled
} from '../../../lib/taskCompleteSound'
import {
  hasActiveTurn,
  peekActiveTurn,
  recordTurnDone,
  recordTurnDoneWithSpan
} from '../../../lib/turnElapsed'
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
import type { StreamHandlerContext } from './types'

type UiToast = Extract<StreamEvent, { kind: 'ui_toast' }>
type ChannelPairingPending = Extract<StreamEvent, { kind: 'channel_pairing_pending' }>
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
type BackgroundJobs = Extract<StreamEvent, { kind: 'background_jobs' }>

export function handleUiToast(ctx: StreamHandlerContext, e: UiToast) {
  if (e.conversationId && e.conversationId !== ctx.currentId.value) return
  const lv = e.level
  const level: 'success' | 'warning' | 'error' =
    lv === 'error' ? 'error' : lv === 'warning' ? 'warning' : 'success'
  ctx.showUiToast(e.message, level)
}

export function handleChannelPairingPending(e: ChannelPairingPending) {
  notifyChannelPairingPending({
    channel: e.channel,
    code: e.code,
    senderId: e.senderId,
    issuedAt: e.issuedAt
  })
}

export function handleToolRoundsExhausted(_ctx: StreamHandlerContext, _e: ToolRoundsExhausted) {
  // Kept for older streams. Tool-round cap is shown once as StreamEvent.Error
  // on the assistant row; do not insert a second 【提示】 bubble.
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
    ctx.showUiToast(t('chat.toast.ephemeralSandboxCreated'), 'warning')
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
    if (e.uiBindings) existing.uiBindings = e.uiBindings
  } else {
    conv.messages.push({
      id: e.messageId,
      role: 'user',
      content: e.content,
      status: 'done',
      createdAt: Date.now(),
      ...(e.attachments?.length ? { attachments: e.attachments } : {}),
      ...(e.uiBindings ? { uiBindings: e.uiBindings } : {})
    })
  }
  conv.messages = dedupeImInboundUserMessages(e.conversationId, conv.messages)
  maybeUpdateConversationTitle(conv)
  conv.updatedAt = Date.now()
  // IM / injected user rows enter memory outside hydrate — stamp creation time.
  ctx.markUserMessageViewed(e.conversationId, e.messageId, Date.now())
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
  // Cancelled run emits Error then Done. Force-send already started the next turn;
  // this Error must not finalize that new turn at ~0s or clear its generating flag.
  if (fallbackConvId && ctx.isStaleStreamAfterInterrupt(fallbackConvId)) {
    console.info('[chat] ignore stale Error after interrupt', { conversationId: fallbackConvId })
    return
  }
  let affectedId: string | null = null
  if (e.messageId) {
    const r = ctx.findMessage(e.messageId, eventConvId || undefined)
    if (r) {
      if (cancelled) {
        // Keep the row so the muted「已停止生成」caption remains visible.
        r.msg.status = 'cancelled'
        r.msg.errorMessage = t('chat.toast.generationStopped')
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
  if (affectedId) {
    // Close turn timing here so a trailing StreamEvent::Done does not chime
    // after cancel / error (docs: no sound on stop or failure).
    if (hasActiveTurn(affectedId)) {
      recordTurnDone(affectedId)
    }
    disarmTaskCompleteAudio()
    ctx.markMetaDirty(affectedId)
  }
}

export function handleDone(ctx: StreamHandlerContext, e: Done) {
  const convId = e.conversationId?.trim() || ctx.currentId.value?.trim() || ''
  // Capture before clearRunState / recordTurnDone.
  // Poll/resync may clear `generating` before Done arrives; turn timing still marks
  // a real user turn so the chime is not skipped after a few minutes of streaming.
  const wasGenerating = !!convId && ctx.isConversationGenerating(convId)
  const activeTurn = convId ? peekActiveTurn(convId) : null
  const hadActiveTurn = !!activeTurn
  const turnId = activeTurn?.turnId ?? null

  // Force-send / interrupt: cancelled run's Done can arrive after the next turn started.
  if (convId && ctx.consumeStaleDoneAfterInterrupt(convId)) {
    console.info('[chat] ignore stale Done after interrupt', { conversationId: convId })
    return
  }

  if (convId && e.backgroundRunningCount != null) {
    ctx.setBackgroundJobCount(convId, e.backgroundRunningCount)
    if (e.backgroundRunningCount <= 0) {
      ctx.reconcileBackgroundHostsWhenOccupancyEmpty(convId)
    }
  }

  try {
    if (convId) {
      // Backend run_chat timestamps are the authoritative work span (excludes
      // frontend queue / network). Missing or rejected span → local dispatch clock.
      if (e.startedAtMs != null && e.finishedAtMs != null && turnId) {
        const span = recordTurnDoneWithSpan(convId, turnId, e.startedAtMs, e.finishedAtMs)
        if (span == null) recordTurnDone(convId)
      } else {
        recordTurnDone(convId)
      }
    }
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
    if (wasGenerating || hadActiveTurn) {
      const jobsStillRunning = !!convId && ctx.hasBackgroundJobs(convId)
      if (!jobsStillRunning) {
        playTaskCompleteSoundIfEnabled(convId, turnId)
      } else {
        console.info('[sound] skip task-complete chime (background jobs still running)', {
          conversationId: convId || null
        })
      }
      // Background finish: solid-dot on sidebar until the user opens this conversation.
      if (convId && convId !== (ctx.currentId.value?.trim() || '')) {
        ctx.markConversationAwaitingView(convId)
      }
    } else {
      console.info('[sound] skip task-complete chime (no generating / active turn)', {
        conversationId: convId || null
      })
    }
  }
}

export function handleBackgroundJobs(ctx: StreamHandlerContext, e: BackgroundJobs) {
  const convId = e.conversationId?.trim()
  if (!convId) return
  ctx.setBackgroundJobCount(convId, e.runningCount, e.jobs ?? (e.runningCount <= 0 ? [] : undefined))
  if (e.runningCount <= 0) {
    ctx.reconcileBackgroundHostsWhenOccupancyEmpty(convId)
  }
}
