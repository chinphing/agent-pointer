import { ensureScopedChildMessage } from '../../../lib/subAgentMessages'
import { ensureSubTrace } from '../../../lib/subAgentSession'
import { closeAbandonedEmptyAssistantShells } from '../helpers'
import type { StreamEvent } from '../../../types/chat'
import type { StreamHandlerContext } from './types'

type SubMessageStart = Extract<StreamEvent, { kind: 'sub_message_start' }>

export function handleSubMessageStart(ctx: StreamHandlerContext, e: SubMessageStart) {
  ctx.ensureImConversation(e.conversationId)
  const found = ctx.findMessage(e.anchorMessageId, e.conversationId)
  const conv = found?.conv ?? ctx.conversations.value.find(c => c.id === e.conversationId)
  const anchor = found?.msg
  if (!conv || !anchor) {
    console.warn('[stream] sub_message_start: anchor message missing', e.anchorMessageId)
    return
  }
  ensureSubTrace(anchor, e.agentInstanceId.trim() || e.traceId, {
    depth: e.spawnDepth,
    agentInstanceId: e.agentInstanceId,
    taskId: e.taskId
  })
  const child = ensureScopedChildMessage(conv, e.anchorMessageId, e.scopedMessageId, {
    traceId: e.traceId,
    taskId: e.taskId,
    spawnDepth: e.spawnDepth,
    agentInstanceId: e.agentInstanceId
  })
  ctx.notifyScopedStreamWrite(conv, anchor, child, e.traceId)
  closeAbandonedEmptyAssistantShells(conv, e.scopedMessageId, {
    agentInstanceId: e.agentInstanceId,
    anchorMessageId: e.anchorMessageId,
    traceId: e.traceId
  })
  anchor.status = 'streaming'
  conv.updatedAt = Date.now()
}
