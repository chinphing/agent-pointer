import type { ChatMessage } from '../types/chat'
import { isScopedSubMessage } from './subAgentMessages'
import { isRealUserTaskMessage } from './threadLayoutGlue'

const STORAGE_KEY = 'pointer.chat.turn-elapsed.v1'

/** Recorded spans below 1s are usually a cancelled-run Error landing on the next turn. */
const MIN_TRUSTED_RECORDED_MS = 1000

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

/**
 * 用后端 run_chat 的权威时间戳覆盖该轮耗时（取代本地 dispatch→Done 计时）。
 * 本地计时包含前端排队/网络传输，而 run_chat 的开始/结束才是 AI 真实工作区间。
 * 仅当该轮仍是当前活动轮次时生效；否则忽略（避免错配）。
 */
export function recordTurnDoneWithSpan(
  conversationId: string,
  turnId: string,
  startedAt: number,
  finishedAt: number
): number | null {
  const state = readState()
  const active = state.active[conversationId]
  if (!active || active.turnId !== turnId) return null
  if (
    !Number.isFinite(startedAt) ||
    !Number.isFinite(finishedAt) ||
    finishedAt < startedAt
  ) {
    return null
  }
  const elapsedMs = finishedAt - startedAt
  state.completed[completedKey(conversationId, turnId)] = elapsedMs
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

/** Real user turn anchor (not compression chip / retry inject / scoped sub-agent stub). */
export function isTurnElapsedAnchorUser(message: ChatMessage): boolean {
  return isRealUserTaskMessage(message) && !isScopedSubMessage(message)
}

/**
 * createdAt window for a user turn: from the anchor through the message before
 * the next real user question. Scoped stubs / env-feedback rows do not close
 * the window (they sit inside the same turn).
 */
export function turnMessageCreatedAtSpan(
  messages: readonly ChatMessage[],
  turnId: string
): { userCreatedAt: number | null; lastMessageCreatedAt: number | null } {
  const userIndex = messages.findIndex(
    message => message.id === turnId && isTurnElapsedAnchorUser(message)
  )
  if (userIndex < 0) return { userCreatedAt: null, lastMessageCreatedAt: null }
  const nextUserOffset = messages
    .slice(userIndex + 1)
    .findIndex(message => isTurnElapsedAnchorUser(message))
  const turnEnd = nextUserOffset >= 0 ? userIndex + 1 + nextUserOffset : messages.length
  const lastMessage = messages[turnEnd - 1]
  return {
    userCreatedAt: messages[userIndex]!.createdAt,
    lastMessageCreatedAt: lastMessage?.createdAt ?? null
  }
}

/**
 * Prefer dispatch→Done timing when present.
 * Message `createdAt` span is only a fallback (e.g. history without local timing);
 * outbound-queue items must use dispatch-time `createdAt` or this fallback includes queue wait.
 * Sub-second recorded values lose to a longer createdAt span (cancelled-run Error
 * often finalizes the next turn at ~0ms before the real Done arrives).
 */
export function resolveTurnElapsedMs(options: {
  conversationId: string | null | undefined
  turnId: string
  userCreatedAt: number | null | undefined
  lastMessageCreatedAt: number | null | undefined
}): number | null {
  const fallback =
    options.userCreatedAt == null || options.lastMessageCreatedAt == null
      ? null
      : elapsedBetweenTimestamps(options.userCreatedAt, options.lastMessageCreatedAt)
  const conversationId = options.conversationId?.trim()
  if (conversationId) {
    const recorded = turnElapsedMs(conversationId, options.turnId)
    if (recorded != null) {
      if (
        recorded < MIN_TRUSTED_RECORDED_MS
        && fallback != null
        && fallback > recorded
      ) {
        return fallback
      }
      return recorded
    }
  }
  return fallback
}

export function formatTurnElapsed(elapsedMs: number | null): string {
  if (elapsedMs == null) return '工作耗时未知'
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = String(totalSeconds % 60).padStart(2, '0')
  return `工作 ${minutes} m ${seconds} s`
}
