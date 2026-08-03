const STORAGE_KEY = 'pointer.chat.turn-elapsed.v1'

interface ActiveTurnTiming {
  turnId: string
  startedAt: number
}

interface TurnElapsedState {
  active: Record<string, ActiveTurnTiming>
  completed: Record<string, number>
}

const memoryState: TurnElapsedState = { active: {}, completed: {} }

function completedKey(conversationId: string, turnId: string): string {
  return `${conversationId}\u0000${turnId}`
}

function readState(): TurnElapsedState {
  if (typeof localStorage === 'undefined') return memoryState
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '') as Partial<TurnElapsedState>
    return {
      active: parsed.active && typeof parsed.active === 'object' ? parsed.active : {},
      completed: parsed.completed && typeof parsed.completed === 'object' ? parsed.completed : {}
    }
  } catch {
    return { active: {}, completed: {} }
  }
}

function writeState(state: TurnElapsedState) {
  memoryState.active = state.active
  memoryState.completed = state.completed
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    // Timing is supplementary UI metadata; storage failure must not block chat.
  }
}

export function peekActiveTurn(conversationId: string): ActiveTurnTiming | null {
  const id = conversationId.trim()
  if (!id) return null
  const active = readState().active[id]
  return active ?? null
}

export function recordTurnStart(conversationId: string, turnId: string, startedAt = Date.now()) {
  const state = readState()
  const prev = state.active[conversationId]
  // Force-send / interrupt starts the next turn before the previous Done arrives.
  // Finalize the interrupted turn so its elapsed is not lost or attributed later.
  if (
    prev &&
    prev.turnId !== turnId &&
    Number.isFinite(prev.startedAt) &&
    startedAt >= prev.startedAt
  ) {
    state.completed[completedKey(conversationId, prev.turnId)] = startedAt - prev.startedAt
  }
  state.active[conversationId] = { turnId, startedAt }
  writeState(state)
}

/** True while a turn is open for this conversation (survives UI generating being cleared early). */
export function hasActiveTurn(conversationId: string): boolean {
  return peekActiveTurn(conversationId) != null
}

export function recordTurnDone(conversationId: string, finishedAt = Date.now()): number | null {
  const state = readState()
  const active = state.active[conversationId]
  if (!active || !Number.isFinite(active.startedAt) || finishedAt < active.startedAt) return null
  const elapsedMs = finishedAt - active.startedAt
  state.completed[completedKey(conversationId, active.turnId)] = elapsedMs
  delete state.active[conversationId]
  writeState(state)
  return elapsedMs
}

export function turnElapsedMs(conversationId: string, turnId: string): number | null {
  const elapsed = readState().completed[completedKey(conversationId, turnId)]
  return Number.isFinite(elapsed) && elapsed >= 0 ? elapsed : null
}

/**
 * When the given turn is currently running, returns its recorded start
 * timestamp (ms epoch) so callers can render a live ticking duration.
 * Returns null when the turn is not the active one or timing was cleared.
 */
export function activeTurnStartedAt(conversationId: string, turnId: string): number | null {
  const id = conversationId.trim()
  if (!id) return null
  const active = readState().active[id]
  if (active && active.turnId === turnId && Number.isFinite(active.startedAt)) {
    return active.startedAt
  }
  return null
}

export function elapsedBetweenTimestamps(startedAt: number, finishedAt: number): number | null {
  if (!Number.isFinite(startedAt) || !Number.isFinite(finishedAt) || finishedAt < startedAt) return null
  return finishedAt - startedAt
}

/**
 * Prefer dispatch→Done timing when present.
 * Message `createdAt` span is only a fallback (e.g. history without local timing);
 * outbound-queue items must use dispatch-time `createdAt` or this fallback includes queue wait.
 */
export function resolveTurnElapsedMs(options: {
  conversationId: string | null | undefined
  turnId: string
  userCreatedAt: number | null | undefined
  lastMessageCreatedAt: number | null | undefined
}): number | null {
  const conversationId = options.conversationId?.trim()
  if (conversationId) {
    const recorded = turnElapsedMs(conversationId, options.turnId)
    if (recorded != null) return recorded
  }
  if (options.userCreatedAt == null || options.lastMessageCreatedAt == null) return null
  return elapsedBetweenTimestamps(options.userCreatedAt, options.lastMessageCreatedAt)
}

export function formatTurnElapsed(elapsedMs: number | null): string {
  if (elapsedMs == null) return '工作耗时未知'
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = String(totalSeconds % 60).padStart(2, '0')
  return `工作 ${minutes} m ${seconds} s`
}
