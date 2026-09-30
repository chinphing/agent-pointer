import type { ToolCall } from '../types/chat'
import { isPendingAskUserToolCall } from './messageTooling'

/**
 * Top-of-chat ask_user banner state (design doc §4).
 *
 * The banner owns the pending-`ask_user` surface: frames / collapsed turns no
 * longer retain anything for ask_user, so the queue must be derived from the
 * lead messages plus the conversation's scoped rows (deep sub-agents included).
 */

/** How long「已选择 X」stays after submit before the banner moves on (D-C3). */
export const ASK_USER_BANNER_LINGER_MS = 2000

export type AskUserBannerLinger = {
  /** The answered tool call (used to keep the confirmation tied to one answer). */
  toolCallId: string
  selected: string[]
  /** Wall clock (ms) when the confirmation stops being shown. */
  expiresAt: number
  /** Pending ids seen at submit time — a brand-new one replaces the linger. */
  knownPendingIds: string[]
}

export type AskUserBannerView =
  | { kind: 'linger'; selected: string[]; remaining: number }
  | { kind: 'ask'; toolCall: ToolCall; remaining: number }

/** Rows that may carry ask_user tool calls (lead messages or scoped rows). */
export type AskUserBannerRow = { toolCalls?: readonly ToolCall[] | undefined }

/**
 * Pending / running `ask_user` tool calls in display order, de-duplicated by id.
 * Works for lead messages and scoped (deep sub-agent) rows alike.
 */
export function pendingAskUserToolCalls(
  rows: readonly AskUserBannerRow[] | undefined
): ToolCall[] {
  const out: ToolCall[] = []
  const seen = new Set<string>()
  for (const row of rows ?? []) {
    for (const tc of row.toolCalls ?? []) {
      if (!isPendingAskUserToolCall(tc)) continue
      const id = tc.id?.trim()
      if (!id || seen.has(id)) continue
      seen.add(id)
      out.push(tc)
    }
  }
  return out
}

/**
 * What the banner should show right now (D-C2 queue + D-C3 linger).
 *
 * - A live linger wins, unless a pending question the user has not seen yet
 *   appeared after the submit — then that one replaces the confirmation.
 * - Otherwise the earliest pending question is shown, with the remaining count.
 */
export function resolveAskUserBannerView(
  pending: readonly ToolCall[],
  linger: AskUserBannerLinger | null,
  now: number
): AskUserBannerView | null {
  if (linger && linger.expiresAt > now) {
    const known = new Set(linger.knownPendingIds)
    const fresh = pending.find(tc => !known.has(tc.id?.trim() ?? ''))
    if (!fresh) {
      return { kind: 'linger', selected: linger.selected, remaining: pending.length }
    }
  }
  const first = pending[0]
  if (!first) return null
  return { kind: 'ask', toolCall: first, remaining: pending.length - 1 }
}

/** Linger record for a just-submitted answer. */
export function createAskUserLinger(options: {
  toolCallId: string
  selected: readonly string[]
  knownPendingIds: readonly string[]
  now: number
  lingerMs?: number
}): AskUserBannerLinger {
  const lingerMs = options.lingerMs ?? ASK_USER_BANNER_LINGER_MS
  return {
    toolCallId: options.toolCallId,
    selected: [...options.selected],
    expiresAt: options.now + lingerMs,
    knownPendingIds: options.knownPendingIds.map(id => id.trim()).filter(Boolean)
  }
}
