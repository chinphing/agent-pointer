import type { OutboundQueueItem } from '../types/chat'

/** Move `itemId` to the front of the FIFO (no-op if missing or already first). */
export function promoteOutboundQueueItem(
  queue: OutboundQueueItem[],
  itemId: string
): OutboundQueueItem[] {
  const idx = queue.findIndex(item => item.id === itemId)
  if (idx < 0) return queue
  if (idx === 0) return queue
  const item = queue[idx]!
  return [item, ...queue.slice(0, idx), ...queue.slice(idx + 1)]
}
