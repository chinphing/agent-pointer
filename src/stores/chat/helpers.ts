import { t } from '../../i18n'
import { isDiscardableEmptyAssistant, assistantHasUserFacingProgress } from '../../lib/assistantMessageKind'
import { leadThreadCompressionInsertIndex } from '../../lib/compressionLayout'
import { isBackgroundJobHost, isBackgroundJobHandleResult, isToolCallInProgress, isLiveBackgroundHostTool, backgroundHandleStatus, isBackgroundHandleInProgress } from '../../lib/toolCallDisplay'
import {
  bindUnboundTracesToHosts,
  isScopedSubMessage,
  isSubAgentHostStubContent
} from '../../lib/subAgentMessages'
import { isSubAgentTraceTerminal } from '../../lib/subAgentSession'
import { randomUuid } from '../../lib/randomUuid'
import type { AgentTrace, ChatMessage, Conversation, ExcludedReason, ToolCall } from '../../types/chat'
import { useConversationScopedStore, type SpawnLookup } from '../../lib/conversationScoped'

/** Conversation / message client ids — UUID v4 (stable opaque segment for media paths). */
export function uid() {
  return randomUuid()
}

function excludedContextState(reason: ExcludedReason): ChatMessage['contextState'] {
  return { included: false, excludedReason: reason }
}

export function applyExcludedMessageIds(
  conv: Conversation,
  messageIds: string[],
  reason: ExcludedReason
) {
  const state = excludedContextState(reason)
  for (const id of messageIds) {
    const msg = conv.messages.find(m => m.id === id)
    if (msg) msg.contextState = state
  }
}

/**
 * Index to splice a compression summary. Use the recorded keep-window id as-is
 * on the lead thread (tool / glue rows included). Do not match scoped
 * sub-agent rows or append after them — that parks the chip under the child.
 * Missing id → first non-excluded lead row, else the first lead row
 * in the loaded window (not after the last lead / live turn).
 */
export function resolveCompressionInsertAt(
  messages: readonly ChatMessage[],
  insertBeforeMessageId: string,
  excludedMessageIds: readonly string[] = []
): number {
  const insertAt = leadThreadCompressionInsertIndex(
    messages,
    insertBeforeMessageId,
    excludedMessageIds
  )
  if (insertAt === messages.length) {
    const anchor = insertBeforeMessageId.trim()
    if (!anchor) {
      console.warn('[chat] compression summary insert: no keep anchor in thread', {
        insertBeforeMessageId: null,
        excluded: excludedMessageIds.length,
        messages: messages.length
      })
    }
  }
  return insertAt
}

export function insertMessageBeforeAnchor(
  conv: Conversation,
  insertBeforeMessageId: string,
  message: ChatMessage,
  excludedMessageIds: readonly string[] = []
) {
  if (conv.messages.some(m => m.id === message.id)) return
  const insertAt = resolveCompressionInsertAt(
    conv.messages,
    insertBeforeMessageId,
    excludedMessageIds
  )
  conv.messages.splice(insertAt, 0, message)
}

export function removeAssistantMessage(conv: Conversation, messageId: string): boolean {
  const idx = conv.messages.findIndex(m => m.id === messageId)
  if (idx < 0) return false
  conv.messages.splice(idx, 1)
  conv.updatedAt = Date.now()
  return true
}

/** Transcript order: SQLite `position`, then `createdAt`, then id. Missing position = in-flight → after persisted rows. */
export function compareMessagesTranscriptOrder(a: ChatMessage, b: ChatMessage): number {
  const aPos = a.position
  const bPos = b.position
  const aMissing = aPos == null
  const bMissing = bPos == null
  if (aMissing !== bMissing) return aMissing ? 1 : -1
  if (!aMissing && !bMissing && aPos !== bPos) return aPos - bPos
  const aAt = a.createdAt ?? 0
  const bAt = b.createdAt ?? 0
  if (aAt !== bAt) return aAt - bAt
  return a.id.localeCompare(b.id)
}

export function sortMessagesInTranscriptOrder(messages: ChatMessage[]): ChatMessage[] {
  if (messages.length < 2) return messages
  return messages.slice().sort(compareMessagesTranscriptOrder)
}

function finiteCreatedAt(messages: readonly ChatMessage[]): number[] {
  return messages
    .map(m => m.createdAt)
    .filter((t): t is number => typeof t === 'number' && Number.isFinite(t) && t > 0)
}

function finitePositions(messages: readonly ChatMessage[]): number[] {
  return messages
    .map(m => m.position)
    .filter((p): p is number => typeof p === 'number' && Number.isFinite(p))
}

/** Still receiving stream tokens, waiting to start, or running tools. */
export function messageIsLiveGenerating(msg: ChatMessage | null | undefined): boolean {
  if (!msg) return false
  if (msg.status === 'streaming' || msg.status === 'pending' || msg.contentStreaming) return true
  // Status may briefly read `done` after a lost generating flag / soft-cancel edge,
  // while tools are still in flight — treat as live so hydrate does not clobber them.
  if (msg.role === 'assistant' && hasInFlightToolCalls(msg)) return true
  return false
}

/**
 * Keep only rows that belong after the current *page window*.
 * Bottom prefetch must never pull earlier turns (low createdAt / low position).
 *
 * `afterPosition` is the paging cursor (`newestPosition`). Do not use max(in-memory
 * positions): a live generating turn spliced onto an around window sits at the
 * real tail and would make every mid-page look "older".
 */
export function retainIncomingNewerMessages(
  existing: readonly ChatMessage[],
  incoming: ChatMessage[],
  options?: { afterPosition?: number | null }
): ChatMessage[] {
  if (incoming.length === 0) return incoming
  const existingTimes = finiteCreatedAt(existing)
  const minTime = existingTimes.length ? Math.min(...existingTimes) : null
  const cursor = options?.afterPosition
  const maxPos =
    cursor != null && Number.isFinite(cursor)
      ? cursor
      : (() => {
          const existingPos = finitePositions(existing)
          return existingPos.length ? Math.max(...existingPos) : null
        })()
  return incoming.filter(m => {
    if (minTime != null && m.createdAt != null && m.createdAt > 0 && m.createdAt < minTime) {
      return false
    }
    if (maxPos != null && m.position != null && m.position <= maxPos) {
      return false
    }
    return true
  })
}

export type HydrateMergeMode = 'keep-older' | 'in-flight-tail'

function overlayLiveStreamingRow(dbMsg: ChatMessage, live: ChatMessage): ChatMessage {
  const longer = (a?: string, b?: string) =>
    (a?.length ?? 0) >= (b?.length ?? 0) ? a : b
  return {
    ...dbMsg,
    ...live,
    content: longer(live.content, dbMsg.content) ?? '',
    reasoning: longer(live.reasoning, dbMsg.reasoning),
    rawContent: longer(live.rawContent, dbMsg.rawContent),
    thoughts: longer(live.thoughts, dbMsg.thoughts),
    toolCalls:
      (live.toolCalls?.length ?? 0) >= (dbMsg.toolCalls?.length ?? 0)
        ? live.toolCalls
        : dbMsg.toolCalls,
    attachments:
      (live.attachments?.length ?? 0) >= (dbMsg.attachments?.length ?? 0)
        ? live.attachments
        : dbMsg.attachments,
    agentTrace:
      (live.agentTrace?.length ?? 0) >= (dbMsg.agentTrace?.length ?? 0)
        ? live.agentTrace
        : dbMsg.agentTrace
  }
}

/**
 * Live extras that belong to the in-flight generating turn (user + streaming
 * assistants after the last live row), not the rest of an around / old-tail window.
 */
function isLeadTurnUser(msg: ChatMessage): boolean {
  if (msg.role !== 'user') return false
  if (isScopedSubMessage(msg)) return false
  if (isSubAgentHostStubContent(msg.content)) return false
  return true
}

export function liveGeneratingTurnExtras(
  inMemory: readonly ChatMessage[],
  dbIds: ReadonlySet<string>
): ChatMessage[] {
  const ordered = sortMessagesInTranscriptOrder(inMemory.slice())
  let lastInFlightIdx = -1
  for (let i = ordered.length - 1; i >= 0; i -= 1) {
    if (messageIsLiveGenerating(ordered[i])) {
      lastInFlightIdx = i
      break
    }
  }
  if (lastInFlightIdx < 0) {
    return ordered.filter(m => !dbIds.has(m.id) && m.position == null)
  }
  let start = lastInFlightIdx
  for (let i = lastInFlightIdx; i >= 0; i -= 1) {
    if (isLeadTurnUser(ordered[i]!)) {
      start = i
      break
    }
  }
  const slice = ordered.slice(start)
  const anchors = new Set(
    slice.map(m => m.anchorMessageId?.trim()).filter((id): id is string => !!id)
  )
  return ordered.filter((m, idx) => {
    if (dbIds.has(m.id)) return false
    if (idx >= start) return true
    return anchors.has(m.id)
  })
}

/**
 * Around-window + live tail: page cursor still in the hole, but memory already
 * holds a row with a SQLite position past that cursor.
 *
 * Do **not** treat positionless live generating rows as disconnected — that is
 * normal on the real tail while streaming. Treating them as disconnected made
 * every switch-back `jumpToLatest` + force-reload, which dropped the middle of
 * the painted window and left the fisheye stuck mid-rail.
 */
export function hasDisconnectedLiveTail(
  messages: readonly ChatMessage[],
  newestPosition: number | null | undefined
): boolean {
  if (newestPosition == null || !Number.isFinite(newestPosition)) return false
  return messages.some(
    m => m.position != null && Number.isFinite(m.position) && m.position > newestPosition
  )
}

/** Re-open / jump must load the real tail, not pin a hole or a poisoned cursor. */
export function conversationNeedsTailReload(
  hasMoreNewer: boolean,
  messages: readonly ChatMessage[],
  newestPosition: number | null | undefined
): boolean {
  if (hasMoreNewer) return true
  return hasDisconnectedLiveTail(messages, newestPosition)
}

export type PageWindowCursor = {
  hasMoreNewer: boolean
  newestPosition: number | null | undefined
}

/**
 * Messages the list may paint for the current page window.
 * An around / hole window is a contiguous SQLite range. Live generating
 * rows (or positions) past `newestPosition` stay in memory for force-tail
 * overlay but must not render in the hole — that mix is what made the
 * transcript look shuffled.
 */
export function messagesInCurrentPageWindow(
  messages: readonly ChatMessage[],
  page: PageWindowCursor | null | undefined
): ChatMessage[] {
  if (!messages.length) return []
  if (!page?.hasMoreNewer) return messages as ChatMessage[]
  const newest = page.newestPosition
  if (newest == null || !Number.isFinite(newest)) return messages as ChatMessage[]
  return messages.filter(m => {
    if (m.position != null && Number.isFinite(m.position)) {
      return m.position <= newest
    }
    return !messageIsLiveGenerating(m)
  })
}

/**
 * Overlay live streaming rows onto a DB page.
 *
 * `keep-older`: SSE catch-up / tail hydrate after the user loaded earlier
 * turns — keep those extra in-memory rows.
 * `in-flight-tail`: around hole or force-tail during a run — keep only the
 * generating turn, not the middle window that would sit before the tail.
 */
export function mergeHydratedMessages(
  inMemory: ChatMessage[],
  fromDb: ChatMessage[],
  mode: HydrateMergeMode = 'keep-older'
): ChatMessage[] {
  if (inMemory.length === 0) return fromDb
  const dbById = new Map(fromDb.map(m => [m.id, m]))
  const merged: ChatMessage[] = []
  for (const dbMsg of fromDb) {
    const live = inMemory.find(m => m.id === dbMsg.id)
    if (live && messageIsLiveGenerating(live)) {
      merged.push(overlayLiveStreamingRow(dbMsg, live))
    } else {
      merged.push(dbMsg)
    }
  }
  const extras =
    mode === 'in-flight-tail'
      ? liveGeneratingTurnExtras(inMemory, new Set(dbById.keys()))
      : inMemory.filter(live => !dbById.has(live.id))
  for (const live of extras) {
    if (!dbById.has(live.id) && !merged.some(m => m.id === live.id)) merged.push(live)
  }
  return sortMessagesInTranscriptOrder(merged)
}

export function mergeMessagePage(
  existing: ChatMessage[],
  incoming: ChatMessage[],
  direction: 'older' | 'newer',
  options?: { afterPosition?: number | null }
): ChatMessage[] {
  if (incoming.length === 0) return existing
  const seen = new Set(existing.map(m => m.id))
  const unique = incoming.filter(m => {
    if (seen.has(m.id)) return false
    seen.add(m.id)
    return true
  })
  if (unique.length === 0) return existing
  if (direction === 'newer') {
    const retained = retainIncomingNewerMessages(existing, unique, options)
    if (retained.length === 0) {
      console.info(
        '[chat] mergeMessagePage: dropped older page requested as newer; bottom prefetch must not pull earlier turns'
      )
      return existing
    }
    return sortMessagesInTranscriptOrder([...existing, ...retained])
  }
  return sortMessagesInTranscriptOrder([...unique, ...existing])
}

export function removeTrailingDiscardableEmptyAssistant(conv: Conversation): boolean {
  const last = conv.messages[conv.messages.length - 1]
  if (!last || !isDiscardableEmptyAssistant(last)) return false
  conv.messages.pop()
  conv.updatedAt = Date.now()
  return true
}

/** Close empty streaming shells that a later MessageStart replaced (overflow retry). */
export function closeAbandonedEmptyAssistantShells(
  conv: Conversation,
  keepId: string,
  scopedLookup?: SpawnLookup
): void {
  let closed = 0
  const visit = (m: ChatMessage) => {
    if (m.id === keepId) return
    if (m.role !== 'assistant') return
    if (m.status !== 'streaming' && m.status !== 'pending') return
    if (assistantHasUserFacingProgress(m)) return
    m.status = 'done'
    m.contentStreaming = false
    closed += 1
  }
  for (const m of conv.messages) visit(m)
  // Scoped overflow only looks at this spawn — never listRows of every agent.
  if (scopedLookup) {
    const scopedStore = useConversationScopedStore()
    for (const m of scopedStore.getRows(conv.id, scopedLookup)) {
      visit(m)
      scopedStore.touchRow(conv.id, m.id)
    }
  }
  if (closed > 0) {
    console.info('[chat] closed abandoned empty assistant shells', {
      conversationId: conv.id,
      keepId,
      closed
    })
    conv.updatedAt = Date.now()
  }
}

/**
 * After user stop without a messageId, keep the trailing assistant row and mark it
 * cancelled so the muted「已停止生成」caption remains visible.
 */
export function markTrailingAssistantCancelled(conv: Conversation): boolean {
  const last = conv.messages[conv.messages.length - 1]
  if (!last || last.role !== 'assistant') return false
  if (last.status === 'done' || last.status === 'error') return false
  last.status = 'cancelled'
  last.errorMessage = t('chat.stopped')
  last.contentStreaming = false
  conv.updatedAt = Date.now()
  return true
}

export function hasInFlightToolCalls(msg: ChatMessage): boolean {
  return (
    msg.toolCalls?.some(
      t =>
        t.status === 'running' ||
        t.status === 'pending' ||
        t.status === 'pending_approval'
    ) ?? false
  )
}

function isLiveBackgroundHost(tc: ToolCall): boolean {
  return isLiveBackgroundHostTool(tc)
}

function isTerminalToolStatus(status: ToolCall['status']): boolean {
  return status === 'success' || status === 'failed' || status === 'rejected'
}

/** Message ids that still show a live background host in memory. */
export function liveBackgroundHostMessageIds(conv: Conversation): string[] {
  const ids: string[] = []
  for (const msg of conv.messages) {
    if ((msg.toolCalls ?? []).some(isLiveBackgroundHost)) ids.push(msg.id)
  }
  return ids
}

/**
 * Occupancy is already 0, but UI hosts may still be `running` because the
 * finish `ToolCallStatus` never reached memory. Copy terminal status/handle
 * from persisted rows (same message + toolCall id). Does not invent cancel.
 */
function rewriteHandleStatus(result: string | undefined, status: string): string | undefined {
  if (!isBackgroundJobHandleResult(result)) return result
  try {
    const v = JSON.parse(result!) as Record<string, unknown>
    v.status = status
    return JSON.stringify(v)
  } catch {
    return result
  }
}

function tracesForHost(msg: ChatMessage, hostId: string): AgentTrace[] {
  const id = hostId.trim()
  return (msg.agentTrace ?? []).filter(t => (t.parentToolCallId ?? '').trim() === id)
}

/**
 * Disk may still say `running` after a stale short-list sync. If every bound
 * child trace is already terminal, promote the host so reload does not show
 * 「后台执行中」 or later 「已取消」.
 */
export function repairBackgroundHostsFromChildOutcomes(conv: Conversation): number {
  let n = 0
  for (const msg of conv.messages) {
    if (msg.role !== 'assistant') continue
    bindUnboundTracesToHosts(msg)
    for (const tc of msg.toolCalls ?? []) {
      if (!isLiveBackgroundHost(tc)) continue
      if (isBackgroundHandleInProgress(tc.result)) continue
      const kids = tracesForHost(msg, tc.id)
      if (kids.length === 0) continue
      if (!kids.every(t => isSubAgentTraceTerminal(t.status))) continue
      const failed = kids.some(t => {
        const s = (t.status || '').trim().toLowerCase()
        return s === 'failed' || s === 'cancelled' || s === 'canceled'
      })
      tc.status = failed ? 'failed' : 'success'
      if (failed) {
        if (!tc.error) tc.error = 'failed'
        tc.result = rewriteHandleStatus(tc.result, 'failed')
      } else {
        delete tc.error
        tc.result = rewriteHandleStatus(tc.result, 'completed')
      }
      n += 1
    }
    if (!hasInFlightToolCalls(msg) && (msg.status === 'streaming' || msg.status === 'pending')) {
      msg.status = 'done'
      msg.contentStreaming = false
    }
  }
  return n
}

export function applyPersistedBackgroundHostOutcomes(
  conv: Conversation,
  persisted: readonly ChatMessage[]
): number {
  if (persisted.length === 0) return 0
  const byId = new Map(persisted.map(m => [m.id, m]))
  let n = 0
  for (const msg of conv.messages) {
    if (msg.role !== 'assistant') continue
    const dbMsg = byId.get(msg.id)
    if (!dbMsg) continue
    for (const tc of msg.toolCalls ?? []) {
      if (!isLiveBackgroundHost(tc)) continue
      const dbTc = dbMsg.toolCalls?.find(t => t.id === tc.id)
      if (!dbTc || !isTerminalToolStatus(dbTc.status)) continue
      tc.status = dbTc.status
      if (dbTc.result !== undefined) tc.result = dbTc.result
      if (dbTc.error !== undefined) tc.error = dbTc.error
      else delete tc.error
      if (dbTc.durationMs !== undefined) tc.durationMs = dbTc.durationMs
      n += 1
    }
  }
  return n
}

function liveBackgroundHostIds(toolCalls: ToolCall[] | undefined): Set<string> {
  const ids = new Set<string>()
  for (const tc of toolCalls ?? []) {
    if (isLiveBackgroundHost(tc)) ids.add(tc.id)
  }
  return ids
}

function finalizeStuckToolCallsList(toolCalls: ToolCall[] | undefined): void {
  for (const tc of toolCalls ?? []) {
    if (
      tc.status !== 'running' &&
      tc.status !== 'pending' &&
      tc.status !== 'pending_approval'
    ) {
      continue
    }
    if (isLiveBackgroundHost(tc)) continue
    if (isBackgroundJobHost(tc) && isBackgroundHandleInProgress(tc.result)) continue
    if (tc.status === 'pending_approval') {
      tc.status = 'rejected'
      continue
    }
    tc.status = tc.result?.trim() ? 'success' : 'failed'
    if (tc.status === 'failed' && !tc.error) tc.error = 'interrupted'
  }
}

function finalizeStuckToolCalls(msg: ChatMessage): void {
  finalizeStuckToolCallsList(msg.toolCalls)
}

/** True only when the assistant turn is still receiving stream events or running tools. */
export function assistantTurnActivelyRunning(msg: ChatMessage): boolean {
  if (msg.role !== 'assistant') return false
  if (msg.status !== 'streaming' && msg.status !== 'pending') return false
  return msg.contentStreaming === true || hasInFlightToolCalls(msg)
}

/**
 * Number of leading messages that may be safely trimmed from an in-memory
 * conversation history, based on per-user-message last-viewed timestamps.
 *
 * Scans user messages from oldest to newest and returns the cut index just
 * before the first user message that should stay: never stamped into the
 * viewport/load map, or viewed within `staleMs`. Rows with a viewedAt older
 * than `staleMs` are eligible to drop. If every stamped user message is
 * stale (no keep pivot), time-based cut allows the whole list — the
 * `minKeepUserTurns` floor still caps how much is removed.
 *
 * Streamed rows without a `position` are never cut, and the new window head
 * is guaranteed to carry a position so the paging cursor stays exact.
 *
 * A floor of `minKeepUserTurns` user messages (default 8 = 1 page × 8 turns)
 * is always kept in memory: time-based trimming never removes more than that,
 * so a freshly-loaded thread is never thinned out below one paging page.
 */
export function computeHistoryTrimCutByViewedAt(
  messages: ChatMessage[],
  viewedAt: ReadonlyMap<string, number>,
  now: number,
  staleMs: number,
  minKeepUserTurns = 8
): number {
  if (messages.length === 0) return 0
  // Time-based scan: cut before the first user message that should stay.
  // Missing stamp → keep (just-loaded race before stamp applies).
  // All stamped + all stale → no pivot; allow cutting the whole list
  // (floor below still keeps the newest N user turns).
  let cutByTime = messages.length
  for (let i = 0; i < messages.length; i += 1) {
    const msg = messages[i]!
    if (msg.role !== 'user') continue
    const viewed = viewedAt.get(msg.id)
    if (viewed == null || now - viewed <= staleMs) {
      cutByTime = i
      break
    }
  }
  // Floor: keep at least `minKeepUserTurns` user turns. Counting from the
  // newest user message backward, the floor index is the start of the window
  // that still holds that many turns; fewer turns than the floor → 0 (no trim).
  let floor = 0
  let userCount = 0
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]!.role === 'user') userCount += 1
    if (userCount === minKeepUserTurns) {
      floor = i
      break
    }
  }
  let cut = Math.min(cutByTime, floor)
  // A streamed / not-yet-persisted row may not have a position; never cut such
  // rows, and never leave a position-less row as the new window head.
  if (cut > 0 && messages[cut]?.position == null) {
    cut = Math.max(0, cut - 1)
  }
  return cut
}

/**
 * Clear stale `streaming`/`pending` flags and stuck tool rows on turns that
 * already finished (e.g. after reload or when revisiting an ended session).
 * Never finalize while tools are still in flight — a lost `generating` flag
 * must not wipe the live tool line on conversation switch.
 */
export function normalizeStaleEndedAssistantTurn(msg: ChatMessage): void {
  if (msg.role !== 'assistant') return
  const inFlight = hasInFlightToolCalls(msg)
  if (inFlight) return
  const looksEnded =
    msg.status === 'done' ||
    ((msg.status === 'streaming' || msg.status === 'pending') && !msg.contentStreaming)
  if (!looksEnded) return
  if (msg.status === 'streaming' || msg.status === 'pending') {
    msg.status = 'done'
  }
  msg.contentStreaming = false
  finalizeStuckToolCalls(msg)
}

function finalizeStuckAgentTraces(msg: ChatMessage): void {
  const liveHosts = liveBackgroundHostIds(msg.toolCalls)
  for (const trace of msg.agentTrace ?? []) {
    const parent = (trace.parentToolCallId || '').trim()
    if (parent && liveHosts.has(parent)) continue
    const st = (trace.status || '').trim()
    if (st === 'running' || st === 'streaming' || st === 'pending') {
      trace.status = 'completed'
    }
    if (trace.session) {
      trace.session.contentStreaming = false
      finalizeStuckToolCallsList(trace.session.toolCalls)
    }
  }
}

/** Host rows still in progress after `run_subagent` `background: true`. */
export function countRunningBackgroundSubagents(conv: Conversation): number {
  let n = 0
  for (const msg of conv.messages) {
    for (const tc of msg.toolCalls ?? []) {
      if (isLiveBackgroundHost(tc)) n += 1
    }
  }
  return n
}

/**
 * JobSupervisor is in-memory: process restart drops jobs, but SQLite may still
 * have host rows at `running`. Occupancy snapshot 0 means the supervisor is
 * empty — reconcile each live host from its handle JSON first. Only treat as
 * interrupted when the handle still claims `running` (true zombie after restart).
 * Never map completed jobs to「已取消」.
 */
export function finalizeOrphanBackgroundHosts(conv: Conversation): number {
  let n = 0
  for (const msg of conv.messages) {
    if (msg.role !== 'assistant') continue
    let changed = 0
    for (const tc of msg.toolCalls ?? []) {
      if (!isLiveBackgroundHost(tc)) continue
      const handleStatus = backgroundHandleStatus(tc.result)
      if (handleStatus === 'completed') {
        tc.status = 'success'
        tc.error = undefined
        changed += 1
        continue
      }
      if (handleStatus === 'failed') {
        tc.status = 'failed'
        if (!tc.error) tc.error = 'failed'
        changed += 1
        continue
      }
      if (handleStatus === 'cancelled' || handleStatus === 'canceled') {
        tc.status = 'failed'
        if (!tc.error) tc.error = 'cancelled'
        changed += 1
        continue
      }
      const kids = tracesForHost(msg, tc.id)
      if (kids.length > 0 && kids.every(t => isSubAgentTraceTerminal(t.status))) {
        const failed = kids.some(t => {
          const s = (t.status || '').trim().toLowerCase()
          return s === 'failed' || s === 'cancelled' || s === 'canceled'
        })
        tc.status = failed ? 'failed' : 'success'
        if (failed) {
          if (!tc.error) tc.error = 'failed'
        } else {
          tc.error = undefined
        }
        changed += 1
        continue
      }
      // Handle missing or still "running" while supervisor occupancy is 0.
      tc.status = 'failed'
      if (!tc.error) tc.error = 'interrupted'
      changed += 1
    }
    if (changed > 0) {
      finalizeStuckAgentTraces(msg)
      n += changed
    }
  }
  return n
}

/** After reload or stop, assistant rows must not stay `streaming`/`pending`. */
export function normalizeInterruptedAssistantStatuses(conversations: Conversation[]): void {
  for (const conv of conversations) {
    for (const m of conv.messages) {
      if (m.role !== 'assistant') continue
      if (m.status === 'streaming' || m.status === 'pending') {
        m.status = 'done'
      }
      m.contentStreaming = false
      finalizeStuckToolCalls(m)
      finalizeStuckAgentTraces(m)
    }
  }
}
