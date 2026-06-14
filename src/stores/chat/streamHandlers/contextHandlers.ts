import { buildCompressionNoticeContent } from '../../../lib/compressionMessage'
import { clearReasoningDeltaBuffer } from '../../../lib/reasoningDeltaBatch'
import type { StreamEvent } from '../../../types/chat'
import { applyExcludedMessageIds, insertMessageBeforeAnchor } from '../helpers'
import type { StreamHandlerContext } from './types'

type ContextTrimApplied = Extract<StreamEvent, { kind: 'context_trim_applied' }>
type ContextCompressionApplied = Extract<StreamEvent, { kind: 'context_compression_applied' }>
type ContextCompressed = Extract<StreamEvent, { kind: 'context_compressed' }>

export function handleContextTrimApplied(ctx: StreamHandlerContext, e: ContextTrimApplied) {
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  applyExcludedMessageIds(conv, e.excludedMessageIds, 'task_board_trim')
  conv.updatedAt = Date.now()
  ctx.persistMeta()
}

export function handleContextCompressionApplied(
  ctx: StreamHandlerContext,
  e: ContextCompressionApplied
) {
  clearReasoningDeltaBuffer()
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  applyExcludedMessageIds(conv, e.excludedMessageIds, 'context_compression')
  const summary = {
    ...e.summaryMessage,
    toolCalls: e.summaryMessage.toolCalls ?? undefined
  }
  insertMessageBeforeAnchor(conv, e.insertBeforeMessageId, summary)
  ctx.showUiToast(buildCompressionNoticeContent(e.compression), 'success')
  conv.updatedAt = Date.now()
  ctx.persistMeta()
  ctx.persistAppend(e.conversationId)
}

export function handleContextCompressed(ctx: StreamHandlerContext, e: ContextCompressed) {
  const r = ctx.findMessage(e.messageId)
  if (!r || r.conv.id !== e.conversationId) return
  const agentId = e.compression.subAgentId
  if (agentId && r.msg.agentTrace?.length) {
    const step = r.msg.agentTrace.find(a => a.id === agentId)
    if (step) {
      const name = e.compression.subAgentName?.trim() || step.name
      step.detail = `${name}：上下文已压缩（${e.compression.droppedCount} 条 → 摘要）`
    }
  }
  ctx.showUiToast(buildCompressionNoticeContent(e.compression), 'success')
  r.conv.updatedAt = Date.now()
}
