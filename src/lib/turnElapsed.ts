import type { ChatMessage } from '../types/chat'
import { t } from '../i18n'
import { isScopedSubMessage } from './subAgentMessages'
import { isRealUserTaskMessage } from './threadLayoutGlue'
import { resolveTurnElapsedPrefix, type TurnElapsedPhase } from './webBranding'

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

/**
 * In-memory only, by design.
 *
 * Finished spans are persisted by the backend on the turn's anchor user message
 * (`messages.turn_elapsed_ms`) and read back from the message after a reload, so
 * this module no longer owns a storage blob. That blob kept one entry per
 * finished turn with no pruning — a long-lived install measured 19k turns /
 * 1.75 MB — and `MessageList` asked for a turn's elapsed time per visible turn
 * per frame, so it was re-read and re-parsed constantly. Dropping it removes the
 * unbounded growth instead of caching around it.
 *
 * `active` stays memory-only too: an in-flight turn only means something to the
 * window that dispatched it, and a stale `active` entry must never survive a
 * reload and render as a ticking label.
 */
const memoryState: TurnElapsedState = { active: {}, completed: {} }

function completedKey(conversationId: string, turnId: string): string {
  return `${conversationId}\u0000${turnId}`
}

/** Test hook: drop every in-memory span (no production caller). */
export function resetTurnElapsedStateForTests(): void {
  memoryState.active = {}
  memoryState.completed = {}
}

export function peekActiveTurn(conversationId: string): ActiveTurnTiming | null {
  const id = conversationId.trim()
  if (!id) return null
  const active = memoryState.active[id]
  return active ?? null
}

export function recordTurnStart(conversationId: string, turnId: string, startedAt = Date.now()) {
  const prev = memoryState.active[conversationId]
  // Force-send / interrupt starts the next turn before the previous Done arrives.
  // Finalize the interrupted turn so its elapsed is not lost or attributed later.
  if (
    prev &&
    prev.turnId !== turnId &&
    Number.isFinite(prev.startedAt) &&
    startedAt >= prev.startedAt
  ) {
    memoryState.completed[completedKey(conversationId, prev.turnId)] =
      startedAt - prev.startedAt
  }
  memoryState.active[conversationId] = { turnId, startedAt }
}

/** True while a turn is open for this conversation (survives UI generating being cleared early). */
export function hasActiveTurn(conversationId: string): boolean {
  return peekActiveTurn(conversationId) != null
}

export function recordTurnDone(conversationId: string, finishedAt = Date.now()): number | null {
  const active = memoryState.active[conversationId]
  if (!active || !Number.isFinite(active.startedAt) || finishedAt < active.startedAt) return null
  const elapsedMs = finishedAt - active.startedAt
  memoryState.completed[completedKey(conversationId, active.turnId)] = elapsedMs
  delete memoryState.active[conversationId]
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
  const active = memoryState.active[conversationId]
  if (!active || active.turnId !== turnId) return null
  if (
    !Number.isFinite(startedAt) ||
    !Number.isFinite(finishedAt) ||
    finishedAt < startedAt
  ) {
    return null
  }
  const elapsedMs = finishedAt - startedAt
  memoryState.completed[completedKey(conversationId, turnId)] = elapsedMs
  delete memoryState.active[conversationId]
  return elapsedMs
}

/** Span recorded in this session (Done handler); `null` after a reload. */
export function turnElapsedMs(conversationId: string, turnId: string): number | null {
  const elapsed = memoryState.completed[completedKey(conversationId, turnId)]
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
  const active = memoryState.active[id]
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
): {
  userCreatedAt: number | null
  lastMessageCreatedAt: number | null
  /** `messages.turn_elapsed_ms` persisted by the backend on the anchor row. */
  recordedTurnElapsedMs: number | null
} {
  const userIndex = messages.findIndex(
    message => message.id === turnId && isTurnElapsedAnchorUser(message)
  )
  if (userIndex < 0) {
    return { userCreatedAt: null, lastMessageCreatedAt: null, recordedTurnElapsedMs: null }
  }
  const nextUserOffset = messages
    .slice(userIndex + 1)
    .findIndex(message => isTurnElapsedAnchorUser(message))
  const turnEnd = nextUserOffset >= 0 ? userIndex + 1 + nextUserOffset : messages.length
  const lastMessage = messages[turnEnd - 1]
  return {
    userCreatedAt: messages[userIndex]!.createdAt,
    lastMessageCreatedAt: lastMessage?.createdAt ?? null,
    recordedTurnElapsedMs: messages[userIndex]!.turnElapsedMs ?? null
  }
}

/**
 * Priority (highest first):
 *
 * 1. **In-session Done span** — `recordTurnDoneWithSpan` from the backend's
 *    `run_chat` timestamps, kept in memory. This is the turn that just finished
 *    in *this* window; it is exact and needs no round trip.
 * 2. **`messages.turn_elapsed_ms`** on the anchor user message — the same span,
 *    persisted by the backend. This is what survives a reload, a restart, or
 *    opening the conversation on another device.
 * 3. **`createdAt` span** — anchor user message → last message of the turn.
 *    Only a derivation: it includes frontend queue / network wait, and it is
 *    all that legacy rows have.
 *
 * A recorded value below 1s loses to a longer `createdAt` span (a cancelled-run
 * Error often finalizes the *next* turn at ~0ms before the real Done arrives),
 * and then the next candidate in the chain is tried.
 */
export function resolveTurnElapsedMs(options: {
  conversationId: string | null | undefined
  turnId: string
  /** `turnElapsedMs` of the turn's anchor message (persisted column). */
  messageTurnElapsedMs?: number | null
  userCreatedAt: number | null | undefined
  lastMessageCreatedAt: number | null | undefined
}): number | null {
  const fallback =
    options.userCreatedAt == null || options.lastMessageCreatedAt == null
      ? null
      : elapsedBetweenTimestamps(options.userCreatedAt, options.lastMessageCreatedAt)
  const conversationId = options.conversationId?.trim()
  const candidates = [
    conversationId ? turnElapsedMs(conversationId, options.turnId) : null,
    options.messageTurnElapsedMs ?? null
  ]
  for (const candidate of candidates) {
    if (candidate == null || !Number.isFinite(candidate) || candidate < 0) continue
    if (candidate < MIN_TRUSTED_RECORDED_MS && fallback != null && fallback > candidate) {
      // Mis-attributed settle: fall through to the next, more trustworthy source.
      continue
    }
    return candidate
  }
  return fallback
}

export function formatTurnElapsed(
  elapsedMs: number | null,
  phase: TurnElapsedPhase = 'done'
): string {
  const prefix = resolveTurnElapsedPrefix(phase)
  if (elapsedMs == null) return t('chat.turnElapsedUnknown', { prefix })
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = String(totalSeconds % 60).padStart(2, '0')
  return t('chat.turnElapsed', { prefix, minutes, seconds })
}
