import type { RunQueueSnapshot } from '../../types/automation'

/** Conversation ids with an active or queued dispatcher run (server truth). */
export function activeConversationIdsFromQueueSnapshot(
  snapshot: RunQueueSnapshot
): Set<string> {
  const ids = new Set<string>()
  for (const lane of snapshot.lanes) {
    if (lane.lane.startsWith('session:') && lane.active > 0) {
      ids.add(lane.lane.slice('session:'.length))
    }
    for (const waiter of lane.waiters) {
      const convId = waiter.conversationId?.trim()
      if (convId) ids.add(convId)
    }
  }
  for (const pending of snapshot.pendingRuns) {
    const convId = pending.conversationId?.trim()
    if (convId) ids.add(convId)
  }
  return ids
}
