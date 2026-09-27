import { t } from '../../../i18n'
import {
  buildCompressionNoticeContent,
} from '../../../lib/compressionMessage'
import { clearStreamDeltaBuffers } from '../../../lib/reasoningDeltaBatch'
import type { StreamEvent } from '../../../types/chat'
import { applyExcludedMessageIds, insertMessageBeforeAnchor } from '../helpers'
import type { StreamHandlerContext } from './types'

type ContextTrimApplied = Extract<StreamEvent, { kind: 'context_trim_applied' }>
type ContextCompressionStarted = Extract<StreamEvent, { kind: 'context_compression_started' }>
type ContextCompressionApplied = Extract<StreamEvent, { kind: 'context_compression_applied' }>
type ContextCompressed = Extract<StreamEvent, { kind: 'context_compressed' }>

function clearContextCompressing(ctx: StreamHandlerContext, conversationId: string) {
  const id = conversationId.trim()
  if (!id) return
  ctx.patchRunState(id, { contextCompressing: null })
}

export function handleContextTrimApplied(ctx: StreamHandlerContext, e: ContextTrimApplied) {
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  applyExcludedMessageIds(conv, e.excludedMessageIds, 'task_board_trim')
  conv.updatedAt = Date.now()
  ctx.markMetaDirty(e.conversationId)
}

export function handleContextCompressionStarted(
  ctx: StreamHandlerContext,
  e: ContextCompressionStarted
) {
  const id = e.conversationId.trim()
  if (!id) {
    console.warn('[chat] context_compression_started missing conversationId')
    return
  }
  ctx.ensureImConversation(id)
  ctx.patchRunState(id, {
    contextCompressing: {
      scope: e.scope || 'main',
      messageId: e.messageId,
      insertBeforeMessageId: e.insertBeforeMessageId,
      subAgentId: e.subAgentId,
      subAgentName: e.subAgentName,
      startedAt: Date.now()
    }
  })
  console.info('[chat] context compression started', {
    conversationId: id,
    scope: e.scope,
    subAgentId: e.subAgentId ?? null
  })
}

export function handleContextCompressionApplied(
  ctx: StreamHandlerContext,
  e: ContextCompressionApplied
) {
  clearStreamDeltaBuffers()
  clearContextCompressing(ctx, e.conversationId)
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  applyExcludedMessageIds(conv, e.excludedMessageIds, 'context_compression')
  const summary = {
    ...e.summaryMessage,
    toolCalls: e.summaryMessage.toolCalls ?? undefined
  }
  insertMessageBeforeAnchor(
    conv,
    e.insertBeforeMessageId,
    summary,
    e.excludedMessageIds
  )
  if (summary.role === 'user' && summary.id) {
    ctx.markUserMessageViewed(e.conversationId, summary.id, Date.now())
  }
  ctx.showUiToast(buildCompressionNoticeContent(e.compression), 'success')
  conv.updatedAt = Date.now()
  ctx.markMetaDirty(e.conversationId)
  ctx.persistAppend(e.conversationId)
}

export function handleContextCompressed(ctx: StreamHandlerContext, e: ContextCompressed) {
  clearContextCompressing(ctx, e.conversationId)
  const r = ctx.findMessage(e.messageId)
  if (!r || r.conv.id !== e.conversationId) return
  const agentId = e.compression.subAgentId
  if (agentId && r.msg.agentTrace?.length) {
    const step = r.msg.agentTrace.find(a => a.id === agentId)
    if (step) {
      const name = e.compression.subAgentName?.trim() || step.name
      step.detail = t('chat.compression.traceDetail', { name, dropped: e.compression.droppedCount })
    }
  }
  ctx.showUiToast(buildCompressionNoticeContent(e.compression), 'success')
  r.conv.updatedAt = Date.now()
}
