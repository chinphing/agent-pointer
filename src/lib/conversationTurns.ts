export type ConversationTurnState = 'completed' | 'active' | 'failed' | 'cancelled'

export interface ConversationTurn<T> {
  id: string
  entries: T[]
  state: ConversationTurnState
  collapsedEntries: T[]
  hiddenCount: number
}

export interface ConversationTurnClassifier<T> {
  key(entry: T): string
  userMessageId(entry: T): string | null
  isActive(entry: T): boolean
  isFailed(entry: T): boolean
  isCancelled(entry: T): boolean
  isSummary(entry: T): boolean
  isDelivery(entry: T): boolean
}

/**
 * Groups display entries into user-anchored turns and derives the compact projection.
 * This is view-only: callers keep expansion state separately and never mutate messages.
 */
export function buildConversationTurns<T>(
  entries: readonly T[],
  classifier: ConversationTurnClassifier<T>
): ConversationTurn<T>[] {
  const groups: Array<{ id: string; entries: T[] }> = []

  for (const entry of entries) {
    const userId = classifier.userMessageId(entry)
    if (userId) {
      groups.push({ id: userId, entries: [entry] })
      continue
    }
    const current = groups[groups.length - 1]
    if (current) {
      current.entries.push(entry)
    } else {
      groups.push({ id: `prelude-${classifier.key(entry)}`, entries: [entry] })
    }
  }

  return groups.map(group => {
    const state: ConversationTurnState = group.entries.some(classifier.isActive)
      ? 'active'
      : group.entries.some(classifier.isCancelled)
        ? 'cancelled'
        : group.entries.some(classifier.isFailed)
          ? 'failed'
          : 'completed'

    // Prelude entries (no user message anchor) always show in full.
    if (!classifier.userMessageId(group.entries[0]!)) {
      return {
        ...group,
        state,
        collapsedEntries: group.entries,
        hiddenCount: 0
      }
    }

    const keep = new Set<T>([group.entries[0]!])
    for (const entry of group.entries) {
      if (classifier.isSummary(entry)) keep.add(entry)
    }
    for (let index = group.entries.length - 1; index > 0; index--) {
      const entry = group.entries[index]!
      if (classifier.isDelivery(entry)) {
        keep.add(entry)
        break
      }
    }
    const collapsedEntries = group.entries.filter(entry => keep.has(entry))
    return {
      ...group,
      state,
      collapsedEntries,
      hiddenCount: group.entries.length - collapsedEntries.length
    }
  })
}

export function shouldAutoExpandTurn<T>(
  turns: readonly ConversationTurn<T>[],
  turnId: string
): boolean {
  const lastTurn = turns[turns.length - 1]
  return lastTurn?.id === turnId
    && lastTurn.hiddenCount > 0
}

export function turnContains<T>(
  turn: ConversationTurn<T>,
  predicate: (entry: T) => boolean
): boolean {
  return turn.entries.some(predicate)
}
