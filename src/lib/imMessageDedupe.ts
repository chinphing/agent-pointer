import type { ChatMessage } from '../types/chat'
import { isImConversation } from './channel-labels'

const CH_INBOUND_PREFIX = 'ch-inbound:'

/** Dedupe key for one IM user turn (stable inbound id or attachment path). */
export function imInboundUserDedupeKey(
  conversationId: string,
  message: ChatMessage
): string | null {
  if (!isImConversation(conversationId) || message.role !== 'user') return null
  if (message.id.startsWith(CH_INBOUND_PREFIX)) return message.id
  const att = message.attachments?.[0]
  const storage = att?.storageRelPath?.trim()
  if (storage) return `att:${storage}`
  const content = message.content?.trim()
  if (content) return `text:${content}`
  return null
}

/** Drop later duplicate IM user rows (e.g. retry before stable ids landed). */
export function dedupeImInboundUserMessages(
  conversationId: string,
  messages: ChatMessage[]
): ChatMessage[] {
  if (!isImConversation(conversationId)) return messages
  const seen = new Set<string>()
  const out: ChatMessage[] = []
  for (const m of messages) {
    if (m.role !== 'user') {
      out.push(m)
      continue
    }
    const key = imInboundUserDedupeKey(conversationId, m)
    if (!key) {
      out.push(m)
      continue
    }
    if (seen.has(key)) continue
    seen.add(key)
    out.push(m)
  }
  return out
}
