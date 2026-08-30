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

/** Server job occupancy. Missing `backgroundJobs` means an older snapshot — skip. */
export function backgroundJobOccupancyFromQueueSnapshot(
  snapshot: RunQueueSnapshot
): Map<string, number> | null {
  if (!snapshot.backgroundJobs) return null
  const map = new Map<string, number>()
  for (const row of snapshot.backgroundJobs) {
    const id = row.conversationId?.trim()
    if (!id) continue
    map.set(id, Math.max(0, Math.floor(Number(row.runningCount) || 0)))
  }
  return map
}
