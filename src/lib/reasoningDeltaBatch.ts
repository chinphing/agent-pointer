/**
 * Batch reactive UI writes for streamed text to reduce Vue re-render churn.
 *
 * ## Two lanes
 *
 * Every channel buffers into two independent lanes:
 *
 * - **fast** — lead (top-level) content. Unchanged constants: reasoning 200 ms /
 *   content 50 ms / tool args 80 ms / tool output 80 ms / web search 80 ms /
 *   json partial 200 ms.
 * - **slow** — sub-agent (scoped) streamed text, {@link SLOW_LANE_BATCH_MS}. A
 *   worker's live line is not latency-sensitive: the user reads it inside a frame
 *   that already ticks ~1/s, so stretching its batch removes ~20 Vue ticks per
 *   second per worker.
 *
 * A key's lane is a **pure function of its identity** (`traceId || scopedMessageId`
 * for the five identity channels, an injected `isScopedMessageId` predicate for
 * `content`), so a key can never migrate between lanes: sub-agent traffic never
 * falls back to the fast lane, by construction — not by a runtime check.
 *
 * ## Escape valves (the only two)
 *
 * 1. **Message end / turn end** — {@link flushStreamDeltaBuffers} flushes both lanes
 *    of all six buffers (message_end, turn done, stream error, interrupt, switch).
 * 2. **ask_user / approval** — the authoritative `tool_call_start` snapshot is applied
 *    synchronously and {@link clearToolArgsDeltaBufferForTool} drops the leftovers in
 *    both lanes, so the question / approval card never waits for the slow timer.
 */

export const REASONING_DELTA_BATCH_MS = 200
export const CONTENT_DELTA_BATCH_MS = 50
export const TOOL_ARGS_DELTA_BATCH_MS = 80
export const TOOL_OUTPUT_DELTA_BATCH_MS = 80
export const WEB_SEARCH_OUTPUT_DELTA_BATCH_MS = 80
/** Latest-wins merge for `assistant_json_partial` (thoughts / toolName / responseText). */
export const ASSISTANT_JSON_PARTIAL_BATCH_MS = 200

/** Slow-lane batch for sub-agent (scoped) streamed text. */
export const SLOW_LANE_BATCH_MS = 1000

/**
 * When many stream keys are pending **in one lane** (fan-out sub-agents), stretch
 * that lane's flush so concurrent workers do not each force a separate Vue tick.
 * Collapsed sub-agent live lines are not visually latency-sensitive — prefer fewer
 * ticks under load.
 *
 * Per lane on purpose: the old global count let six sub-agent keys stretch the
 * **lead's** fast lane to 500 ms, which is exactly the direction that hurts.
 */
export const HIGH_PRESSURE_STREAM_KEYS = 6
export const HIGH_PRESSURE_BATCH_MS = 500

/** Which batch lane a delta belongs to (see the module doc above). */
export type StreamLane = 'fast' | 'slow'

export type StreamDeltaApply = (
  messageId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  text: string
) => void

export type ReasoningDeltaApply = StreamDeltaApply
export type ContentDeltaApply = StreamDeltaApply

/** Tool-call scoped delta apply (args / terminal output / web search output). */
export type ToolDeltaApply = (
  messageId: string,
  toolCallId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  text: string
) => void

export type ToolArgsDeltaApply = ToolDeltaApply
export type ToolOutputDeltaApply = ToolDeltaApply
export type WebSearchOutputDeltaApply = ToolDeltaApply

export type AssistantJsonPartialPatch = {
  thoughts?: string | null
  toolName?: string | null
  responseText?: string | null
}

export type AssistantJsonPartialApply = (
  messageId: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  patch: AssistantJsonPartialPatch
) => void

type LaneState<T> = {
  pending: Map<string, T>
  flushTimer: ReturnType<typeof setTimeout> | undefined
}

type LaneBuffers<T> = Record<StreamLane, LaneState<T>>

type DeltaBuffer<T, A> = {
  lanes: LaneBuffers<T>
  getApplyHandler: () => A | null
  label: string
  defaultBatchMs: number
  /** Apply one merged key (key encoding is channel specific). */
  applyKey: (key: string, batch: T, applyHandler: A) => void
  /** Merge a new delta into a pending entry (append / latest-wins). */
  merge: (prev: T | undefined, next: T) => T
}

function createLaneBuffers<T>(): LaneBuffers<T> {
  return {
    fast: { pending: new Map(), flushTimer: undefined },
    slow: { pending: new Map(), flushTimer: undefined }
  }
}

const LANES: readonly StreamLane[] = ['fast', 'slow']

// --- lane routing -----------------------------------------------------------

/**
 * Injected by `stores/chat.ts`: `true` when `messageId` is a sub-agent (scoped) row.
 *
 * The `content` channel is the one channel whose event carries no identity
 * (`types/chat.ts` `{ kind: 'delta'; messageId; text }`), so it cannot decide its
 * lane from the event alone. **Default `false` (conservative)** — an unregistered or
 * throwing predicate sends the text down the fast lane, so a mis-wired predicate can
 * only ever cost perf, never add latency to the lead.
 */
let scopedMessageIdPredicate: ((messageId: string) => boolean) | null = null

export function setScopedMessageIdPredicate(
  predicate: ((messageId: string) => boolean) | null
): void {
  scopedMessageIdPredicate = predicate
}

function isScopedMessageId(messageId: string): boolean {
  if (!scopedMessageIdPredicate) return false
  try {
    return scopedMessageIdPredicate(messageId) === true
  } catch (error) {
    console.warn('[contentDeltaBatch] scoped message predicate failed', error)
    return false
  }
}

/**
 * Lane for the five identity channels: any sub-agent identity (`traceId` or
 * `scopedMessageId`) means the delta belongs to a worker.
 */
export function streamLaneFor(traceId?: string, scopedMessageId?: string): StreamLane {
  return traceId?.trim() || scopedMessageId?.trim() ? 'slow' : 'fast'
}

/** Lane for the `content` channel — the injected predicate, default fast. */
export function contentLaneFor(messageId: string): StreamLane {
  return isScopedMessageId(messageId) ? 'slow' : 'fast'
}

// --- buffers ----------------------------------------------------------------

const reasoningBuffer: DeltaBuffer<string, StreamDeltaApply> = {
  lanes: createLaneBuffers<string>(),
  getApplyHandler: () => reasoningApplyHandler,
  label: 'reasoningDeltaBatch',
  defaultBatchMs: REASONING_DELTA_BATCH_MS,
  applyKey: (key, text, handler) => applyStreamKey(key, text, handler, 'reasoningDeltaBatch'),
  merge: (prev, next) => (prev ?? '') + next
}
const contentBuffer: DeltaBuffer<string, StreamDeltaApply> = {
  lanes: createLaneBuffers<string>(),
  getApplyHandler: () => contentApplyHandler,
  label: 'contentDeltaBatch',
  defaultBatchMs: CONTENT_DELTA_BATCH_MS,
  applyKey: (key, text, handler) => applyStreamKey(key, text, handler, 'contentDeltaBatch'),
  merge: (prev, next) => (prev ?? '') + next
}
const toolArgsBuffer: DeltaBuffer<string, ToolDeltaApply> = {
  lanes: createLaneBuffers<string>(),
  getApplyHandler: () => toolArgsApplyHandler,
  label: 'toolArgsDeltaBatch',
  defaultBatchMs: TOOL_ARGS_DELTA_BATCH_MS,
  applyKey: (key, text, handler) => applyToolKey(key, text, handler, 'toolArgsDeltaBatch'),
  merge: (prev, next) => (prev ?? '') + next
}
const toolOutputBuffer: DeltaBuffer<string, ToolDeltaApply> = {
  lanes: createLaneBuffers<string>(),
  getApplyHandler: () => toolOutputApplyHandler,
  label: 'toolOutputDeltaBatch',
  defaultBatchMs: TOOL_OUTPUT_DELTA_BATCH_MS,
  applyKey: (key, text, handler) => applyToolKey(key, text, handler, 'toolOutputDeltaBatch'),
  merge: (prev, next) => (prev ?? '') + next
}
const webSearchOutputBuffer: DeltaBuffer<string, ToolDeltaApply> = {
  lanes: createLaneBuffers<string>(),
  getApplyHandler: () => webSearchOutputApplyHandler,
  label: 'webSearchOutputDeltaBatch',
  defaultBatchMs: WEB_SEARCH_OUTPUT_DELTA_BATCH_MS,
  applyKey: (key, text, handler) => applyToolKey(key, text, handler, 'webSearchOutputDeltaBatch'),
  merge: (prev, next) => (prev ?? '') + next
}
const jsonPartialBuffer: DeltaBuffer<AssistantJsonPartialPatch, AssistantJsonPartialApply> = {
  lanes: createLaneBuffers<AssistantJsonPartialPatch>(),
  getApplyHandler: () => jsonPartialApplyHandler,
  label: 'assistantJsonPartialBatch',
  defaultBatchMs: ASSISTANT_JSON_PARTIAL_BATCH_MS,
  applyKey: (key, patch, handler) => applyJsonPartialKey(key, patch, handler),
  merge: mergeJsonPartial
}

let reasoningApplyHandler: ReasoningDeltaApply | null = null
let contentApplyHandler: ContentDeltaApply | null = null
let toolArgsApplyHandler: ToolArgsDeltaApply | null = null
let toolOutputApplyHandler: ToolOutputDeltaApply | null = null
let webSearchOutputApplyHandler: WebSearchOutputDeltaApply | null = null
let jsonPartialApplyHandler: AssistantJsonPartialApply | null = null

/** Pending entries in one lane, summed over the six buffers. */
function lanePendingSize(lane: StreamLane): number {
  return (
    reasoningBuffer.lanes[lane].pending.size
    + contentBuffer.lanes[lane].pending.size
    + toolArgsBuffer.lanes[lane].pending.size
    + toolOutputBuffer.lanes[lane].pending.size
    + webSearchOutputBuffer.lanes[lane].pending.size
    + jsonPartialBuffer.lanes[lane].pending.size
  )
}

/**
 * Entries still buffered across **both lanes** of all six buffers. Read-only: these
 * are released per message (`delete`/`clear`), so a number that only ever grows
 * between flushes means a message stopped being released (see `lib/residencyProbe.ts`).
 */
export function pendingDeltaCount(): number {
  return lanePendingSize('fast') + lanePendingSize('slow')
}

function bufferKey(messageId: string, traceId?: string, scopedMessageId?: string): string {
  const tid = traceId?.trim()
  const sid = scopedMessageId?.trim()
  if (sid) return `${messageId}\0${tid ?? ''}\0${sid}`
  if (tid) return `${messageId}\0${tid}`
  return messageId
}

function toolBufferKey(
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): string {
  const tid = traceId?.trim()
  const sid = scopedMessageId?.trim()
  if (sid) return `${messageId}\0${toolCallId}\0${tid ?? ''}\0${sid}`
  if (tid) return `${messageId}\0${toolCallId}\0${tid}`
  return `${messageId}\0${toolCallId}`
}

/**
 * Delay for one lane. The slow lane floors at {@link SLOW_LANE_BATCH_MS}; the fast
 * lane keeps the caller's requested delay byte-for-byte.
 */
function laneBatchMs(lane: StreamLane, requested: number): number {
  if (lane === 'slow') return Math.max(SLOW_LANE_BATCH_MS, requested)
  return requested
}

function effectiveBatchMs(lane: StreamLane, requested: number): number {
  const base = laneBatchMs(lane, requested)
  if (lanePendingSize(lane) >= HIGH_PRESSURE_STREAM_KEYS) {
    return Math.max(base, HIGH_PRESSURE_BATCH_MS)
  }
  return base
}

export function setReasoningDeltaApplyHandler(handler: ReasoningDeltaApply | null): void {
  reasoningApplyHandler = handler
}

export function setContentDeltaApplyHandler(handler: ContentDeltaApply | null): void {
  contentApplyHandler = handler
}

export function setToolArgsDeltaApplyHandler(handler: ToolArgsDeltaApply | null): void {
  toolArgsApplyHandler = handler
}

export function setToolOutputDeltaApplyHandler(handler: ToolOutputDeltaApply | null): void {
  toolOutputApplyHandler = handler
}

export function setWebSearchOutputDeltaApplyHandler(handler: WebSearchOutputDeltaApply | null): void {
  webSearchOutputApplyHandler = handler
}

export function setAssistantJsonPartialApplyHandler(handler: AssistantJsonPartialApply | null): void {
  jsonPartialApplyHandler = handler
}

function scheduleLaneFlush<T, A>(
  buffer: DeltaBuffer<T, A>,
  lane: StreamLane,
  batchMs: number
): void {
  const state = buffer.lanes[lane]
  if (state.flushTimer != null) return
  state.flushTimer = setTimeout(() => {
    state.flushTimer = undefined
    flushLane(buffer, lane)
  }, effectiveBatchMs(lane, batchMs))
}

function applyStreamKey(
  key: string,
  text: string,
  applyHandler: StreamDeltaApply,
  label: string
): void {
  const parts = key.split('\0')
  try {
    applyHandler(parts[0], parts[1] || undefined, parts[2] || undefined, text)
  } catch (error) {
    console.warn(`[${label}] apply failed`, error)
  }
}

function applyToolKey(
  key: string,
  text: string,
  applyHandler: ToolDeltaApply,
  label: string
): void {
  const parts = key.split('\0')
  try {
    applyHandler(parts[0], parts[1], parts[2] || undefined, parts[3] || undefined, text)
  } catch (error) {
    console.warn(`[${label}] apply failed`, error)
  }
}

function applyJsonPartialKey(
  key: string,
  patch: AssistantJsonPartialPatch,
  applyHandler: AssistantJsonPartialApply
): void {
  const parts = key.split('\0')
  try {
    applyHandler(parts[0], parts[1] || undefined, parts[2] || undefined, patch)
  } catch (error) {
    console.warn('[assistantJsonPartialBatch] apply failed', error)
  }
}

function flushLane<T, A>(
  buffer: DeltaBuffer<T, A>,
  lane: StreamLane,
  messageId?: string
): void {
  const state = buffer.lanes[lane]
  if (state.flushTimer != null) {
    clearTimeout(state.flushTimer)
    state.flushTimer = undefined
  }
  const applyHandler = buffer.getApplyHandler()
  if (!applyHandler) {
    if (state.pending.size > 0) {
      console.warn(`[${buffer.label}] flush skipped: no apply handler registered`)
    }
    if (messageId == null) state.pending.clear()
    else {
      for (const key of [...state.pending.keys()]) {
        if (key.split('\0')[0] === messageId) state.pending.delete(key)
      }
    }
    return
  }
  const keys = [...state.pending.keys()]
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    const batch = state.pending.get(key)
    state.pending.delete(key)
    if (!batch) continue
    buffer.applyKey(key, batch, applyHandler)
  }
  // Partial flush (one messageId) may leave siblings pending — reschedule this lane.
  if (state.pending.size > 0) {
    scheduleLaneFlush(buffer, lane, buffer.defaultBatchMs)
  }
}

/** Flush one buffer in both lanes (all keys, or only `messageId`'s). */
function flushBuffer<T, A>(buffer: DeltaBuffer<T, A>, messageId?: string): void {
  for (const lane of LANES) flushLane(buffer, lane, messageId)
}

function enqueueInto<T, A>(
  buffer: DeltaBuffer<T, A>,
  lane: StreamLane,
  key: string,
  value: T,
  batchMs: number
): void {
  const state = buffer.lanes[lane]
  state.pending.set(key, buffer.merge(state.pending.get(key), value))
  scheduleLaneFlush(buffer, lane, batchMs)
}

function enqueueDelta(
  buffer: DeltaBuffer<string, StreamDeltaApply>,
  messageId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  enqueueInto(
    buffer,
    streamLaneFor(traceId, scopedMessageId),
    bufferKey(messageId, traceId, scopedMessageId),
    text,
    batchMs
  )
}

function enqueueToolDelta(
  buffer: DeltaBuffer<string, ToolDeltaApply>,
  messageId: string,
  toolCallId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  enqueueInto(
    buffer,
    streamLaneFor(traceId, scopedMessageId),
    toolBufferKey(messageId, toolCallId, traceId, scopedMessageId),
    text,
    batchMs
  )
}

function mergeJsonPartial(
  prev: AssistantJsonPartialPatch | undefined,
  next: AssistantJsonPartialPatch
): AssistantJsonPartialPatch {
  return {
    thoughts: next.thoughts !== undefined ? next.thoughts : prev?.thoughts,
    toolName: next.toolName !== undefined ? next.toolName : prev?.toolName,
    responseText: next.responseText !== undefined ? next.responseText : prev?.responseText
  }
}

function clearLaneBuffer<T, A>(buffer: DeltaBuffer<T, A>): void {
  for (const lane of LANES) {
    const state = buffer.lanes[lane]
    if (state.flushTimer != null) clearTimeout(state.flushTimer)
    state.flushTimer = undefined
    state.pending.clear()
  }
}

export function enqueueReasoningDelta(
  messageId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = REASONING_DELTA_BATCH_MS
): void {
  enqueueDelta(reasoningBuffer, messageId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueContentDelta(
  messageId: string,
  text: string,
  batchMs: number = CONTENT_DELTA_BATCH_MS
): void {
  enqueueInto(contentBuffer, contentLaneFor(messageId), bufferKey(messageId), text, batchMs)
}

export function enqueueToolArgsDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = TOOL_ARGS_DELTA_BATCH_MS
): void {
  enqueueToolDelta(toolArgsBuffer, messageId, toolCallId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueToolOutputDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = TOOL_OUTPUT_DELTA_BATCH_MS
): void {
  enqueueToolDelta(toolOutputBuffer, messageId, toolCallId, text, traceId, scopedMessageId, batchMs)
}

export function enqueueWebSearchOutputDelta(
  messageId: string,
  toolCallId: string,
  text: string,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = WEB_SEARCH_OUTPUT_DELTA_BATCH_MS
): void {
  enqueueToolDelta(
    webSearchOutputBuffer,
    messageId,
    toolCallId,
    text,
    traceId,
    scopedMessageId,
    batchMs
  )
}

export function enqueueAssistantJsonPartial(
  messageId: string,
  patch: AssistantJsonPartialPatch,
  traceId?: string,
  scopedMessageId?: string,
  batchMs: number = ASSISTANT_JSON_PARTIAL_BATCH_MS
): void {
  if (
    patch.thoughts === undefined
    && patch.toolName === undefined
    && patch.responseText === undefined
  ) {
    return
  }
  enqueueInto(
    jsonPartialBuffer,
    streamLaneFor(traceId, scopedMessageId),
    bufferKey(messageId, traceId, scopedMessageId),
    patch,
    batchMs
  )
}

/** Flush pending reasoning for one message (all traces) or the entire buffer. */
export function flushReasoningDeltaBuffer(messageId?: string): void {
  flushBuffer(reasoningBuffer, messageId)
}

/** Flush pending assistant body text for one message or the entire buffer. */
export function flushContentDeltaBuffer(messageId?: string): void {
  flushBuffer(contentBuffer, messageId)
}

/** Flush pending tool call args for one message or the entire buffer. */
export function flushToolArgsDeltaBuffer(messageId?: string): void {
  flushBuffer(toolArgsBuffer, messageId)
}

/** Flush pending terminal output for one message or the entire buffer. */
export function flushToolOutputDeltaBuffer(messageId?: string): void {
  flushBuffer(toolOutputBuffer, messageId)
}

/** Flush pending web search output for one message or the entire buffer. */
export function flushWebSearchOutputDeltaBuffer(messageId?: string): void {
  flushBuffer(webSearchOutputBuffer, messageId)
}

/** Flush pending assistant JSON partials for one message or the entire buffer. */
export function flushAssistantJsonPartialBuffer(messageId?: string): void {
  flushBuffer(jsonPartialBuffer, messageId)
}

/**
 * Escape valve 1 — message end / turn end. Flushes **both lanes** of every channel,
 * so a slow-lane worker body can never outlive its message or turn.
 */
export function flushStreamDeltaBuffers(messageId?: string): void {
  flushReasoningDeltaBuffer(messageId)
  flushContentDeltaBuffer(messageId)
  flushToolArgsDeltaBuffer(messageId)
  flushToolOutputDeltaBuffer(messageId)
  flushWebSearchOutputDeltaBuffer(messageId)
  flushAssistantJsonPartialBuffer(messageId)
}

export function clearReasoningDeltaBuffer(): void {
  clearLaneBuffer(reasoningBuffer)
}

export function clearContentDeltaBuffer(): void {
  clearLaneBuffer(contentBuffer)
}

export function clearToolArgsDeltaBuffer(): void {
  clearLaneBuffer(toolArgsBuffer)
}

/**
 * Drop pending batched args for one tool call without applying them — **both lanes**.
 *
 * Needed when a later `tool_call_start` carries the authoritative full `arguments`
 * string — otherwise a deferred flush would append chunks that were already included
 * in that snapshot (duplicate / trailing junk). This is also escape valve 2's tool
 * half: an `ask_user` question or an approval card always arrives as such a snapshot,
 * so it never waits for the slow timer.
 */
export function clearToolArgsDeltaBufferForTool(
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): void {
  const key = toolBufferKey(messageId, toolCallId, traceId, scopedMessageId)
  toolArgsBuffer.lanes.fast.pending.delete(key)
  toolArgsBuffer.lanes.slow.pending.delete(key)
}

export function clearToolOutputDeltaBuffer(): void {
  clearLaneBuffer(toolOutputBuffer)
}

export function clearWebSearchOutputDeltaBuffer(): void {
  clearLaneBuffer(webSearchOutputBuffer)
}

export function clearAssistantJsonPartialBuffer(): void {
  clearLaneBuffer(jsonPartialBuffer)
}

export function clearStreamDeltaBuffers(): void {
  clearReasoningDeltaBuffer()
  clearContentDeltaBuffer()
  clearToolArgsDeltaBuffer()
  clearToolOutputDeltaBuffer()
  clearWebSearchOutputDeltaBuffer()
  clearAssistantJsonPartialBuffer()
}
