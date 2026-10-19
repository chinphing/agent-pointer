import type { ChatMessage } from '../types/chat'
import { t } from '../i18n'
import { isScopedSubMessage } from './subAgentMessages'
import { isRealUserTaskMessage } from './threadLayoutGlue'
import { resolveTurnElapsedPrefix, type TurnElapsedPhase } from './webBranding'

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

/**
 * Raw blob last folded into `memoryState`; `undefined` means "unknown, read the
 * storage again".
 *
 * Why the cache: this blob holds one entry per finished turn and is never
 * pruned, so on a long-lived install it reaches ~877k characters (1.75 MB in
 * WebKit's UTF-16 storage). `MessageList` renders `turnElapsedLabel()` per
 * visible turn per frame, and every call used to `JSON.parse` that whole blob —
 * a real-device HUD window measured 955 parses / 745.7M characters / 2.0s of
 * main thread in 24s of streaming, i.e. the parse of *unchanged* data was the
 * stall. Parsing is now keyed by the raw string: an unchanged blob costs one
 * `getItem` + a string compare, and a write (or another window's write) still
 * invalidates the cache exactly as a fresh read would.
 */
let cachedRaw: string | undefined

function completedKey(conversationId: string, turnId: string): string {
  return `${conversationId}\u0000${turnId}`
}

/**
 * Current state. Returns the shared `memoryState` (not a copy): callers that
 * mutate it are the recorders, and they always `writeState` right after, so the
 * cache never drifts from what is on disk. A copy would re-allocate every turn
 * entry on every render — the cost this cache exists to remove.
 */
function readState(): TurnElapsedState {
  if (typeof localStorage === 'undefined') return memoryState
  let raw: string | undefined
  try {
    raw = localStorage.getItem(STORAGE_KEY) ?? ''
    if (raw === cachedRaw) return memoryState
    const parsed = JSON.parse(raw) as Partial<TurnElapsedState>
    cachedRaw = raw
    memoryState.active = parsed.active && typeof parsed.active === 'object' ? parsed.active : {}
    memoryState.completed =
      parsed.completed && typeof parsed.completed === 'object' ? parsed.completed : {}
    return memoryState
  } catch {
    // Unreadable / corrupt storage: same empty state as before. Remember the
    // exact raw we failed on so the next render does not parse it again; a
    // `getItem` failure leaves `raw` undefined and is retried.
    cachedRaw = raw
    memoryState.active = {}
    memoryState.completed = {}
    return memoryState
  }
}

function writeState(state: TurnElapsedState) {
  memoryState.active = state.active
  memoryState.completed = state.completed
  if (typeof localStorage === 'undefined') return
  try {
    const raw = JSON.stringify(state)
    localStorage.setItem(STORAGE_KEY, raw)
    cachedRaw = raw
  } catch {
    // Timing is supplementary UI metadata; storage failure must not block chat.
    // Memory may not match disk, so the next read must go back to storage.
    cachedRaw = undefined
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
