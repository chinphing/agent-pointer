import { ensureScopedChildMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace } from '../../../lib/subAgentSession'
import type { StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type SubMessageStart = Extract<StreamEvent, { kind: 'sub_message_start' }>

export function handleSubMessageStart(ctx: StreamHandlerContext, e: SubMessageStart) {
  ctx.ensureImConversation(e.conversationId)
  const conv = ctx.conversations.value.find(c => c.id === e.conversationId)
  if (!conv) return
  const anchor = conv.messages.find(m => m.id === e.anchorMessageId)
  if (!anchor) {
    console.warn('[stream] sub_message_start: anchor message missing', e.anchorMessageId)
    return
  }
  ensureSubTrace(anchor, e.traceId, { depth: e.spawnDepth })
  ensureScopedChildMessage(conv, e.anchorMessageId, e.scopedMessageId, {
    traceId: e.traceId,
    taskId: e.taskId,
    spawnDepth: e.spawnDepth
  })
  anchor.status = 'streaming'
  conv.updatedAt = Date.now()
}
