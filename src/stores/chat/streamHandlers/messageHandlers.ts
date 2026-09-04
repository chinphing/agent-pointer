import {
  enqueueAssistantJsonPartial,
  enqueueContentDelta,
  enqueueReasoningDelta,
  flushStreamDeltaBuffers,
  type AssistantJsonPartialPatch
} from '../../../lib/reasoningDeltaBatch'
import { isPlannerPhaseThoughts } from '../../../lib/plannerPhase'
import { maybeUpdateConversationTitle } from '../../../lib/conversationTitle'
import { toolCallBaseName } from '../../../lib/messageTooling'
import { resolveStreamWriteMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace, ensureSubTraceSession } from '../../../lib/subAgentSession'
import { assistantHasVisibleProgress } from '../../../lib/assistantMessageKind'
import { closeAbandonedEmptyAssistantShells } from '../helpers'
import type { ChatMessage, StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type MessageStart = Extract<StreamEvent, { kind: 'message_start' }>
type Delta = Extract<StreamEvent, { kind: 'delta' }>
type RawContentDelta = Extract<StreamEvent, { kind: 'raw_content_delta' }>
type ReasoningDelta = Extract<StreamEvent, { kind: 'reasoning_delta' }>
type AssistantJsonPartial = Extract<StreamEvent, { kind: 'assistant_json_partial' }>
type MessageEnd = Extract<StreamEvent, { kind: 'message_end' }>

type TraceScopedEvent = {
  traceId?: string
  scopedMessageId?: string
}

function markAssistantStreaming(msg: { status: ChatMessage['status']; contentStreaming?: boolean }) {
  if (msg.status === 'cancelled' || msg.status === 'error') return
  if (msg.status === 'streaming' && msg.contentStreaming === true) return
  msg.status = 'streaming'
  msg.contentStreaming = true
}

function alreadyAssistantStreaming(msg: {
  status: ChatMessage['status']
  contentStreaming?: boolean
}): boolean {
  return msg.status === 'streaming' && msg.contentStreaming === true
}

function applyAssistantJsonPartialToMessage(msg: ChatMessage, e: AssistantJsonPartial) {
  markAssistantStreaming(msg)
  if (e.thoughts != null) {
    if (e.thoughts.trim() !== '') msg.thoughts = e.thoughts
    else if (isPlannerPhaseThoughts(msg.thoughts)) delete msg.thoughts
  }
  if (e.toolName != null && e.toolName.trim() !== '') {
    msg.toolNamePreview = e.toolName
    if (e.toolName.trim() !== 'response') delete msg.responseTextDraft
  }
  if (e.responseText !== undefined) {
    const t = e.responseText ?? ''
    if (t.trim() !== '') msg.responseTextDraft = t
    else delete msg.responseTextDraft
  }
}

function applyAssistantJsonPartialLegacySession(
  session: NonNullable<ReturnType<typeof ensureSubTraceSession>>,
  e: AssistantJsonPartial
) {
  session.contentStreaming = true
  if (e.thoughts != null) {
    if (e.thoughts.trim() !== '') session.thoughts = e.thoughts
    else if (isPlannerPhaseThoughts(session.thoughts)) delete session.thoughts
  }
  if (e.toolName != null && e.toolName.trim() !== '') {
    session.toolNamePreview = e.toolName
    if (e.toolName.trim() !== 'response') delete session.responseTextDraft
  }
  if (e.responseText !== undefined) {
    const t = e.responseText ?? ''
    if (t.trim() !== '') session.responseTextDraft = t
    else delete session.responseTextDraft
  }
}

function resolveScopedOrLegacy(
  ctx: StreamHandlerContext,
  r: { conv: import('../../../types/chat').Conversation; msg: ChatMessage },
  e: TraceScopedEvent
): ChatMessage | null {
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) return target
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    return ensureSubTraceSession(trace) as unknown as ChatMessage
  }
  return r.msg
}

export function handleMessageStart(ctx: StreamHandlerContext, e: MessageStart) {
  ctx.ensureImConversation(e.conversationId)
  ctx.ensureCronStreamConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  ctx.patchRunState(e.conversationId, { generating: true, activeMessageId: e.messageId })
  closeAbandonedEmptyAssistantShells(conv, e.messageId)
  const existing = conv.messages.find(m => m.id === e.messageId)
  if (!existing) {
    conv.messages.push({
      id: e.messageId,
      role: 'assistant',
      content: '',
      status: 'streaming',
      contentStreaming: true,
      createdAt: Date.now(),
      toolCalls: []
    })
  } else {
    existing.status = 'streaming'
    existing.contentStreaming = true
  }
}

export function handleDelta(_ctx: StreamHandlerContext, e: Delta) {
  enqueueContentDelta(e.messageId, e.text)
}

export function handleRawContentDelta(ctx: StreamHandlerContext, e: RawContentDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  // rawContent is UI-only debug data; skip capture when the raw-content view is off.
  const capture = ctx.rawContentCaptureEnabled()
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    if (capture) {
      target.rawContent = (target.rawContent || '') + e.text
      markAssistantStreaming(target)
    } else if (!alreadyAssistantStreaming(target)) {
      // Fan-out sub-agents emit raw deltas every token; avoid dirtying Vue when
      // already streaming and the debug panel is off.
      markAssistantStreaming(target)
    }
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = ensureSubTraceSession(trace)
    if (capture) session.rawContent = (session.rawContent || '') + e.text
    if (capture || session.contentStreaming !== true) session.contentStreaming = true
    return
  }
  if (capture) {
    r.msg.rawContent = (r.msg.rawContent || '') + e.text
    markAssistantStreaming(r.msg)
  } else if (!alreadyAssistantStreaming(r.msg)) {
    markAssistantStreaming(r.msg)
  }
}

export function handleReasoningDelta(_ctx: StreamHandlerContext, e: ReasoningDelta) {
  enqueueReasoningDelta(e.messageId, e.text, e.traceId, e.scopedMessageId)
}

export function handleAssistantJsonPartial(_ctx: StreamHandlerContext, e: AssistantJsonPartial) {
  const patch: AssistantJsonPartialPatch = {}
  if (e.thoughts !== undefined) patch.thoughts = e.thoughts
  if (e.toolName !== undefined) patch.toolName = e.toolName
  if (e.responseText !== undefined) patch.responseText = e.responseText
  enqueueAssistantJsonPartial(e.messageId, patch, e.traceId, e.scopedMessageId)
}

/** Applied from the batched flush (also used by tests that call apply paths directly). */
export function applyAssistantJsonPartialEvent(
  ctx: StreamHandlerContext,
  messageId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  patch: AssistantJsonPartialPatch
) {
  const r = ctx.findMessage(messageId)
  if (!r) return
  const synthetic: AssistantJsonPartial = {
    kind: 'assistant_json_partial',
    messageId,
    thoughts: patch.thoughts ?? undefined,
    toolName: patch.toolName ?? undefined,
    responseText: patch.responseText ?? undefined,
    traceId,
    scopedMessageId
  }
  const target = resolveStreamWriteMessage(r.conv, r.msg, traceId, scopedMessageId)
  if (target) {
    applyAssistantJsonPartialToMessage(target, synthetic)
    ctx.notifyScopedStreamWrite(r.conv, r.msg, target, traceId)
    return
  }
  if (traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, traceId.trim())
    applyAssistantJsonPartialLegacySession(ensureSubTraceSession(trace), synthetic)
    ctx.notifyScopedStreamWrite(r.conv, r.msg, null, traceId)
    return
  }
  applyAssistantJsonPartialToMessage(r.msg, synthetic)
}

export function handleMessageEnd(ctx: StreamHandlerContext, e: MessageEnd) {
  flushStreamDeltaBuffers(e.messageId)
  const r = ctx.findMessage(e.messageId)
  if (r) {
    if (maybeUpdateConversationTitle(r.conv)) {
      r.conv.updatedAt = Date.now()
    }
    const scopedTarget = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
    // Only short-circuit for scoped sub-agent rows. Lead messages also resolve to
    // `scopedTarget === r.msg`; they must fall through so attachments and run finish
    // logic apply (MEDIA: delivery depends on `e.attachments`).
    if (scopedTarget && e.scopedMessageId?.trim()) {
      scopedTarget.contentStreaming = false
      if (e.content != null) scopedTarget.content = e.content
      if (e.rawContent != null && ctx.rawContentCaptureEnabled()) scopedTarget.rawContent = e.rawContent
      // toolRawOutput is debug-only and must not accumulate on UI messages.
      if (e.attachments?.length) scopedTarget.attachments = e.attachments
      if (e.thoughts != null && e.thoughts.trim() !== '') scopedTarget.thoughts = e.thoughts
      delete scopedTarget.toolNamePreview
      delete scopedTarget.responseTextDraft
      // Do not clobber user-stop (`cancelled`) or hard errors with `done`.
      if (
        !ctx.isConversationGenerating(r.conv.id) &&
        scopedTarget.status !== 'cancelled' &&
        scopedTarget.status !== 'error'
      ) {
        scopedTarget.status = 'done'
      }
      ctx.notifyScopedStreamWrite(r.conv, r.msg, scopedTarget, e.traceId)
      r.conv.updatedAt = Date.now()
      return
    }
    if (e.traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, e.traceId.trim())
      const session = trace.session
      if (session) session.contentStreaming = false
      return
    }
    const terminalMediaDelivery =
      (e.attachments?.length ?? 0) > 0 && !ctx.hasInFlightToolCalls(r.msg)
    // Late message_end after Stop must not wipe `cancelled` (or the inline caption disappears).
    // Empty overflow/retry shells must not stay `streaming` while generating continues —
    // that freezes「思考中.」on the abandoned row.
    if (r.msg.status !== 'cancelled' && r.msg.status !== 'error') {
      const emptyRetryShell = !e.content && !assistantHasVisibleProgress(r.msg)
      r.msg.status =
        !emptyRetryShell && ctx.isConversationGenerating(r.conv.id) && !terminalMediaDelivery
          ? 'streaming'
          : 'done'
    }
    r.msg.contentStreaming = false
    const preview = r.msg.toolNamePreview?.trim()
    const draft = r.msg.responseTextDraft?.trim()
    if (draft && preview && toolCallBaseName(preview) === 'response') {
      r.msg.content = draft
    }
    delete r.msg.toolNamePreview
    if (e.content != null) r.msg.content = e.content
    if (e.attachments?.length) r.msg.attachments = e.attachments
    if (e.rawContent != null && ctx.rawContentCaptureEnabled()) r.msg.rawContent = e.rawContent
    // toolRawOutput is debug-only and must not accumulate on UI messages.
    delete r.msg.responseTextDraft
    if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
    r.conv.updatedAt = Date.now()
  }
  if (r) ctx.markMetaDirty(r.conv.id)
}
