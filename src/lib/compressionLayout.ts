import type { ChatMessage } from '../types/chat'
import { isScopedSubMessage } from './subAgentMessages'

/**
 * Insert index for a parent-thread compression chip.
 * Honor `insertBeforeMessageId` on the lead thread only — scoped sub-agent
 * rows are skipped so a child id (or appending after children) cannot park
 * the chip under the current sub-agent. Missing id → first non-excluded
 * lead row, else the first lead row in this list (loaded window start).
 * Never append after the last lead — that parks the chip on the live turn.
 */
export function leadThreadCompressionInsertIndex(
  messages: readonly ChatMessage[],
  insertBeforeMessageId: string,
  excludedMessageIds: readonly string[] = []
): number {
  const anchor = insertBeforeMessageId.trim()
  if (anchor) {
    const idx = messages.findIndex(m => m.id === anchor && !isScopedSubMessage(m))
    if (idx >= 0) return idx
  }

  const excluded = new Set(
    excludedMessageIds.map(id => id.trim()).filter(id => id.length > 0)
  )
  if (excluded.size > 0) {
    const firstKeptLead = messages.findIndex(
      m => !isScopedSubMessage(m) && !excluded.has(m.id)
    )
    if (firstKeptLead >= 0) return firstKeptLead
  }

  const firstLead = messages.findIndex(m => !isScopedSubMessage(m))
  if (firstLead >= 0) return firstLead
  return messages.length
}
