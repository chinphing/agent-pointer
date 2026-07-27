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

export function recordTurnStart(conversationId: string, turnId: string, startedAt = Date.now()) {
  const state = readState()
  state.active[conversationId] = { turnId, startedAt }
  writeState(state)
}

/** True while a turn is open for this conversation (survives UI generating being cleared early). */
export function hasActiveTurn(conversationId: string): boolean {
  const id = conversationId.trim()
  if (!id) return false
  return !!readState().active[id]
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

export function elapsedBetweenTimestamps(startedAt: number, finishedAt: number): number | null {
  if (!Number.isFinite(startedAt) || !Number.isFinite(finishedAt) || finishedAt < startedAt) return null
  return finishedAt - startedAt
}

export function formatTurnElapsed(elapsedMs: number | null): string {
  if (elapsedMs == null) return '工作耗时未知'
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = String(totalSeconds % 60).padStart(2, '0')
  return `工作 ${minutes} m ${seconds} s`
}
