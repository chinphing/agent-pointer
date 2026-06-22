import { enqueueReasoningDelta, flushReasoningDeltaBuffer } from '../../../lib/reasoningDeltaBatch'
import { maybeUpdateConversationTitle } from '../../../lib/conversationTitle'
import { stripOutboundMediaMarkers } from '../../../lib/outboundMedia'
import { toolCallBaseName } from '../../../lib/messageTooling'
import { resolveStreamWriteMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace, ensureSubTraceSession } from '../../../lib/subAgentSession'
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

function applyAssistantJsonPartialToMessage(msg: ChatMessage, e: AssistantJsonPartial) {
  msg.contentStreaming = true
  msg.status = 'streaming'
  if (e.thoughts != null && e.thoughts.trim() !== '') msg.thoughts = e.thoughts
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
  if (e.thoughts != null && e.thoughts.trim() !== '') session.thoughts = e.thoughts
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
  ctx.patchRunState(e.conversationId, { generating: true, activeMessageId: e.messageId })
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
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

export function handleDelta(ctx: StreamHandlerContext, e: Delta) {
  const r = ctx.findMessage(e.messageId)
  if (r) {
    r.msg.content += e.text
    r.msg.status = 'streaming'
    r.msg.contentStreaming = true
  }
}

export function handleRawContentDelta(ctx: StreamHandlerContext, e: RawContentDelta) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    target.rawContent = (target.rawContent || '') + e.text
    target.contentStreaming = true
    target.status = 'streaming'
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = ensureSubTraceSession(trace)
    session.rawContent = (session.rawContent || '') + e.text
    session.contentStreaming = true
    return
  }
  r.msg.rawContent = (r.msg.rawContent || '') + e.text
  r.msg.status = 'streaming'
  r.msg.contentStreaming = true
}

export function handleReasoningDelta(_ctx: StreamHandlerContext, e: ReasoningDelta) {
  enqueueReasoningDelta(e.messageId, e.text, e.traceId, e.scopedMessageId)
}

export function handleAssistantJsonPartial(ctx: StreamHandlerContext, e: AssistantJsonPartial) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  const target = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
  if (target) {
    applyAssistantJsonPartialToMessage(target, e)
    return
  }
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    applyAssistantJsonPartialLegacySession(ensureSubTraceSession(trace), e)
    return
  }
  applyAssistantJsonPartialToMessage(r.msg, e)
}

export function handleMessageEnd(ctx: StreamHandlerContext, e: MessageEnd) {
  flushReasoningDeltaBuffer(e.messageId)
  const r = ctx.findMessage(e.messageId)
  if (r) {
    if (maybeUpdateConversationTitle(r.conv)) {
      r.conv.updatedAt = Date.now()
    }
    const scopedTarget = resolveStreamWriteMessage(r.conv, r.msg, e.traceId, e.scopedMessageId)
    if (scopedTarget) {
      scopedTarget.contentStreaming = false
      if (e.content != null) scopedTarget.content = stripOutboundMediaMarkers(e.content)
      if (e.rawContent != null) scopedTarget.rawContent = e.rawContent
      if (e.thoughts != null && e.thoughts.trim() !== '') scopedTarget.thoughts = e.thoughts
      delete scopedTarget.toolNamePreview
      delete scopedTarget.responseTextDraft
      if (!ctx.isConversationGenerating(r.conv.id)) {
        scopedTarget.status = 'done'
      }
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
    r.msg.status =
      ctx.isConversationGenerating(r.conv.id) && !terminalMediaDelivery
        ? 'streaming'
        : 'done'
    r.msg.contentStreaming = false
    const preview = r.msg.toolNamePreview?.trim()
    const draft = r.msg.responseTextDraft?.trim()
    if (draft && preview && toolCallBaseName(preview) === 'response') {
      r.msg.content = stripOutboundMediaMarkers(draft)
    }
    delete r.msg.toolNamePreview
    if (e.content != null) r.msg.content = stripOutboundMediaMarkers(e.content)
    if (e.attachments?.length) r.msg.attachments = e.attachments
    if (e.rawContent != null) r.msg.rawContent = e.rawContent
    if (e.toolRawOutput != null) r.msg.toolRawOutput = e.toolRawOutput
    delete r.msg.responseTextDraft
    if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
    r.conv.updatedAt = Date.now()
    ctx.scheduleMaybeFinishGenerating(r.conv.id, e.messageId)
  }
  ctx.persistMeta()
}
