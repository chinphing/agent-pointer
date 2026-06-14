import { enqueueReasoningDelta, flushReasoningDeltaBuffer } from '../../../lib/reasoningDeltaBatch'
import { imConversationTitle, isImConversation } from '../../../lib/channel-labels'
import { stripOutboundMediaMarkers } from '../../../lib/outboundMedia'
import { toolCallBaseName } from '../../../lib/messageTooling'
import { ensureSubTrace } from '../../../lib/subAgentSession'
import type { StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type MessageStart = Extract<StreamEvent, { kind: 'message_start' }>
type Delta = Extract<StreamEvent, { kind: 'delta' }>
type RawContentDelta = Extract<StreamEvent, { kind: 'raw_content_delta' }>
type ReasoningDelta = Extract<StreamEvent, { kind: 'reasoning_delta' }>
type AssistantJsonPartial = Extract<StreamEvent, { kind: 'assistant_json_partial' }>
type MessageEnd = Extract<StreamEvent, { kind: 'message_end' }>

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
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    const session = trace.session!
    session.rawContent = (session.rawContent || '') + e.text
    session.contentStreaming = true
  } else {
    r.msg.rawContent = (r.msg.rawContent || '') + e.text
    r.msg.status = 'streaming'
    r.msg.contentStreaming = true
  }
}

export function handleReasoningDelta(_ctx: StreamHandlerContext, e: ReasoningDelta) {
  enqueueReasoningDelta(e.messageId, e.text, e.traceId)
}

function applyAssistantJsonPartialToSession(
  session: NonNullable<ReturnType<typeof ensureSubTrace>['session']>,
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

export function handleAssistantJsonPartial(ctx: StreamHandlerContext, e: AssistantJsonPartial) {
  const r = ctx.findMessage(e.messageId)
  if (!r) return
  if (e.traceId?.trim()) {
    const trace = ensureSubTrace(r.msg, e.traceId.trim())
    applyAssistantJsonPartialToSession(trace.session!, e)
  } else {
    r.msg.status = 'streaming'
    r.msg.contentStreaming = true
    if (e.thoughts != null && e.thoughts.trim() !== '') r.msg.thoughts = e.thoughts
    if (e.toolName != null && e.toolName.trim() !== '') {
      r.msg.toolNamePreview = e.toolName
      if (e.toolName.trim() !== 'response') delete r.msg.responseTextDraft
    }
    if (e.responseText !== undefined) {
      const t = e.responseText ?? ''
      if (t.trim() !== '') r.msg.responseTextDraft = t
      else delete r.msg.responseTextDraft
    }
  }
}

export function handleMessageEnd(ctx: StreamHandlerContext, e: MessageEnd) {
  flushReasoningDeltaBuffer(e.messageId)
  const r = ctx.findMessage(e.messageId)
  if (r) {
    if (e.traceId?.trim()) {
      const trace = ensureSubTrace(r.msg, e.traceId.trim())
      if (trace.session) trace.session.contentStreaming = false
    } else {
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
      if (r.conv.title === '新会话') {
        const firstUser = r.conv.messages.find(m => m.role === 'user')
        if (isImConversation(r.conv.id)) {
          r.conv.title = imConversationTitle(r.conv.id, {
            firstUserText: firstUser?.content
          })
        } else if (firstUser) {
          r.conv.title = firstUser.content.slice(0, 24) || '新会话'
        }
      }
      ctx.scheduleMaybeFinishGenerating(r.conv.id, e.messageId)
    }
  }
  ctx.persistMeta()
}
