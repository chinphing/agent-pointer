/** Batch reactive UI writes for streamed text to reduce Vue re-render churn. */

export const REASONING_DELTA_BATCH_MS = 200
export const CONTENT_DELTA_BATCH_MS = 50
export const TOOL_ARGS_DELTA_BATCH_MS = 80
export const TOOL_OUTPUT_DELTA_BATCH_MS = 80
export const WEB_SEARCH_OUTPUT_DELTA_BATCH_MS = 80
/** Latest-wins merge for `assistant_json_partial` (thoughts / toolName / responseText). */
export const ASSISTANT_JSON_PARTIAL_BATCH_MS = 200
/**
 * When many stream keys are pending (fan-out sub-agents), stretch the shared flush
 * so concurrent workers do not each force a separate Vue tick. Collapsed sub-agent
 * live lines are not visually latency-sensitive — prefer fewer ticks under load.
 */
export const HIGH_PRESSURE_STREAM_KEYS = 6
export const HIGH_PRESSURE_BATCH_MS = 500

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

type DeltaBuffer = {
  pending: Map<string, string>
  flushTimer: ReturnType<typeof setTimeout> | undefined
  getApplyHandler: () => StreamDeltaApply | null
  label: string
  defaultBatchMs: number
}

type ToolDeltaBuffer = {
  pending: Map<string, string>
  flushTimer: ReturnType<typeof setTimeout> | undefined
  getApplyHandler: () => ToolDeltaApply | null
  label: string
  defaultBatchMs: number
}

type JsonPartialBuffer = {
  pending: Map<string, AssistantJsonPartialPatch>
  flushTimer: ReturnType<typeof setTimeout> | undefined
  getApplyHandler: () => AssistantJsonPartialApply | null
  label: string
  defaultBatchMs: number
}

const reasoningPending = new Map<string, string>()
const contentPending = new Map<string, string>()
const toolArgsPending = new Map<string, string>()
const toolOutputPending = new Map<string, string>()
const webSearchOutputPending = new Map<string, string>()
const jsonPartialPending = new Map<string, AssistantJsonPartialPatch>()

let reasoningApplyHandler: ReasoningDeltaApply | null = null
let contentApplyHandler: ContentDeltaApply | null = null
let toolArgsApplyHandler: ToolArgsDeltaApply | null = null
let toolOutputApplyHandler: ToolOutputDeltaApply | null = null
let webSearchOutputApplyHandler: WebSearchOutputDeltaApply | null = null
let jsonPartialApplyHandler: AssistantJsonPartialApply | null = null

/**
 * Entries still buffered across all six pending maps. Read-only: these are
 * released per message (`delete`/`clear`), so a number that only ever grows
 * between flushes means a message stopped being released (see `lib/residencyProbe.ts`).
 */
export function pendingDeltaCount(): number {
  return (
    reasoningPending.size +
    contentPending.size +
    toolArgsPending.size +
    toolOutputPending.size +
    webSearchOutputPending.size +
    jsonPartialPending.size
  )
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

function pendingStreamKeyCount(): number {
  return (
    reasoningPending.size
    + contentPending.size
    + toolArgsPending.size
    + toolOutputPending.size
    + webSearchOutputPending.size
    + jsonPartialPending.size
  )
}

function effectiveBatchMs(requested: number): number {
  if (pendingStreamKeyCount() >= HIGH_PRESSURE_STREAM_KEYS) {
    return Math.max(requested, HIGH_PRESSURE_BATCH_MS)
  }
  return requested
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

function scheduleSharedFlush(
  getTimer: () => ReturnType<typeof setTimeout> | undefined,
  setTimer: (t: ReturnType<typeof setTimeout> | undefined) => void,
  flushAll: () => void,
  batchMs: number
): void {
  if (getTimer() != null) return
  setTimer(setTimeout(() => {
    setTimer(undefined)
    flushAll()
  }, effectiveBatchMs(batchMs)))
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

function flushAllDeltaKeys(buffer: DeltaBuffer, messageId?: string): void {
  if (buffer.flushTimer != null) {
    clearTimeout(buffer.flushTimer)
    buffer.flushTimer = undefined
  }
  const applyHandler = buffer.getApplyHandler()
  if (!applyHandler) {
    if (buffer.pending.size > 0) {
      console.warn(`[${buffer.label}] flush skipped: no apply handler registered`)
    }
    if (messageId == null) buffer.pending.clear()
    else {
      for (const key of [...buffer.pending.keys()]) {
        if (key.split('\0')[0] === messageId) buffer.pending.delete(key)
      }
    }
    return
  }
  const keys = [...buffer.pending.keys()]
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    const batch = buffer.pending.get(key)
    buffer.pending.delete(key)
    if (!batch) continue
    applyStreamKey(key, batch, applyHandler, buffer.label)
  }
  // Partial flush (one messageId) may leave siblings pending — reschedule.
  if (buffer.pending.size > 0) {
    scheduleSharedFlush(
      () => buffer.flushTimer,
      t => {
        buffer.flushTimer = t
      },
      () => flushAllDeltaKeys(buffer),
      buffer.defaultBatchMs
    )
  }
}

function flushAllToolKeys(buffer: ToolDeltaBuffer, messageId?: string): void {
  if (buffer.flushTimer != null) {
    clearTimeout(buffer.flushTimer)
    buffer.flushTimer = undefined
  }
  const applyHandler = buffer.getApplyHandler()
  if (!applyHandler) {
    if (buffer.pending.size > 0) {
      console.warn(`[${buffer.label}] flush skipped: no apply handler registered`)
    }
    if (messageId == null) buffer.pending.clear()
    else {
      for (const key of [...buffer.pending.keys()]) {
        if (key.split('\0')[0] === messageId) buffer.pending.delete(key)
      }
    }
    return
  }
  const keys = [...buffer.pending.keys()]
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    const batch = buffer.pending.get(key)
    buffer.pending.delete(key)
    if (!batch) continue
    applyToolKey(key, batch, applyHandler, buffer.label)
  }
  if (buffer.pending.size > 0) {
    scheduleSharedFlush(
      () => buffer.flushTimer,
      t => {
        buffer.flushTimer = t
      },
      () => flushAllToolKeys(buffer),
      buffer.defaultBatchMs
    )
  }
}

function enqueueDelta(
  buffer: DeltaBuffer,
  messageId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  const key = bufferKey(messageId, traceId, scopedMessageId)
  buffer.pending.set(key, (buffer.pending.get(key) ?? '') + text)
  scheduleSharedFlush(
    () => buffer.flushTimer,
    t => {
      buffer.flushTimer = t
    },
    () => flushAllDeltaKeys(buffer),
    batchMs
  )
}

function enqueueToolDelta(
  buffer: ToolDeltaBuffer,
  messageId: string,
  toolCallId: string,
  text: string,
  traceId: string | undefined,
  scopedMessageId: string | undefined,
  batchMs: number
): void {
  if (!text) return
  const key = toolBufferKey(messageId, toolCallId, traceId, scopedMessageId)
  buffer.pending.set(key, (buffer.pending.get(key) ?? '') + text)
  scheduleSharedFlush(
    () => buffer.flushTimer,
    t => {
      buffer.flushTimer = t
    },
    () => flushAllToolKeys(buffer),
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

function clearDeltaBuffer(buffer: DeltaBuffer): void {
  if (buffer.flushTimer != null) clearTimeout(buffer.flushTimer)
  buffer.flushTimer = undefined
  buffer.pending.clear()
}

function clearToolDeltaBuffer(buffer: ToolDeltaBuffer): void {
  if (buffer.flushTimer != null) clearTimeout(buffer.flushTimer)
  buffer.flushTimer = undefined
  buffer.pending.clear()
}

const reasoningBuffer: DeltaBuffer = {
  pending: reasoningPending,
  flushTimer: undefined,
  getApplyHandler: () => reasoningApplyHandler,
  label: 'reasoningDeltaBatch',
  defaultBatchMs: REASONING_DELTA_BATCH_MS
}
const contentBuffer: DeltaBuffer = {
  pending: contentPending,
  flushTimer: undefined,
  getApplyHandler: () => contentApplyHandler,
  label: 'contentDeltaBatch',
  defaultBatchMs: CONTENT_DELTA_BATCH_MS
}
const toolArgsBuffer: ToolDeltaBuffer = {
  pending: toolArgsPending,
  flushTimer: undefined,
  getApplyHandler: () => toolArgsApplyHandler,
  label: 'toolArgsDeltaBatch',
  defaultBatchMs: TOOL_ARGS_DELTA_BATCH_MS
}
const toolOutputBuffer: ToolDeltaBuffer = {
  pending: toolOutputPending,
  flushTimer: undefined,
  getApplyHandler: () => toolOutputApplyHandler,
  label: 'toolOutputDeltaBatch',
  defaultBatchMs: TOOL_OUTPUT_DELTA_BATCH_MS
}
const webSearchOutputBuffer: ToolDeltaBuffer = {
  pending: webSearchOutputPending,
  flushTimer: undefined,
  getApplyHandler: () => webSearchOutputApplyHandler,
  label: 'webSearchOutputDeltaBatch',
  defaultBatchMs: WEB_SEARCH_OUTPUT_DELTA_BATCH_MS
}
const jsonPartialBuffer: JsonPartialBuffer = {
  pending: jsonPartialPending,
  flushTimer: undefined,
  getApplyHandler: () => jsonPartialApplyHandler,
  label: 'assistantJsonPartialBatch',
  defaultBatchMs: ASSISTANT_JSON_PARTIAL_BATCH_MS
}

function flushAllJsonPartialKeys(messageId?: string): void {
  if (jsonPartialBuffer.flushTimer != null) {
    clearTimeout(jsonPartialBuffer.flushTimer)
    jsonPartialBuffer.flushTimer = undefined
  }
  const applyHandler = jsonPartialBuffer.getApplyHandler()
  if (!applyHandler) {
    if (jsonPartialBuffer.pending.size > 0) {
      console.warn(`[${jsonPartialBuffer.label}] flush skipped: no apply handler registered`)
    }
    if (messageId == null) jsonPartialBuffer.pending.clear()
    else {
      for (const key of [...jsonPartialBuffer.pending.keys()]) {
        if (key.split('\0')[0] === messageId) jsonPartialBuffer.pending.delete(key)
      }
    }
    return
  }
  const keys = [...jsonPartialBuffer.pending.keys()]
  for (const key of keys) {
    if (messageId != null && key.split('\0')[0] !== messageId) continue
    const patch = jsonPartialBuffer.pending.get(key)
    jsonPartialBuffer.pending.delete(key)
    if (!patch) continue
    const parts = key.split('\0')
    try {
      applyHandler(parts[0], parts[1] || undefined, parts[2] || undefined, patch)
    } catch (error) {
      console.warn(`[${jsonPartialBuffer.label}] apply failed`, error)
    }
  }
  if (jsonPartialBuffer.pending.size > 0) {
    scheduleSharedFlush(
      () => jsonPartialBuffer.flushTimer,
      t => {
        jsonPartialBuffer.flushTimer = t
      },
      () => flushAllJsonPartialKeys(),
      jsonPartialBuffer.defaultBatchMs
    )
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
  enqueueDelta(contentBuffer, messageId, text, undefined, undefined, batchMs)
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
  const key = bufferKey(messageId, traceId, scopedMessageId)
  jsonPartialBuffer.pending.set(key, mergeJsonPartial(jsonPartialBuffer.pending.get(key), patch))
  scheduleSharedFlush(
    () => jsonPartialBuffer.flushTimer,
    t => {
      jsonPartialBuffer.flushTimer = t
    },
    () => flushAllJsonPartialKeys(),
    batchMs
  )
}

/** Flush pending reasoning for one message (all traces) or the entire buffer. */
export function flushReasoningDeltaBuffer(messageId?: string): void {
  flushAllDeltaKeys(reasoningBuffer, messageId)
}

/** Flush pending assistant body text for one message or the entire buffer. */
export function flushContentDeltaBuffer(messageId?: string): void {
  flushAllDeltaKeys(contentBuffer, messageId)
}

/** Flush pending tool call args for one message or the entire buffer. */
export function flushToolArgsDeltaBuffer(messageId?: string): void {
  flushAllToolKeys(toolArgsBuffer, messageId)
}

/** Flush pending terminal output for one message or the entire buffer. */
export function flushToolOutputDeltaBuffer(messageId?: string): void {
  flushAllToolKeys(toolOutputBuffer, messageId)
}

/** Flush pending web search output for one message or the entire buffer. */
export function flushWebSearchOutputDeltaBuffer(messageId?: string): void {
  flushAllToolKeys(webSearchOutputBuffer, messageId)
}

/** Flush pending assistant JSON partials for one message or the entire buffer. */
export function flushAssistantJsonPartialBuffer(messageId?: string): void {
  flushAllJsonPartialKeys(messageId)
}

export function flushStreamDeltaBuffers(messageId?: string): void {
  flushReasoningDeltaBuffer(messageId)
  flushContentDeltaBuffer(messageId)
  flushToolArgsDeltaBuffer(messageId)
  flushToolOutputDeltaBuffer(messageId)
  flushWebSearchOutputDeltaBuffer(messageId)
  flushAssistantJsonPartialBuffer(messageId)
}

export function clearReasoningDeltaBuffer(): void {
  clearDeltaBuffer(reasoningBuffer)
}

export function clearContentDeltaBuffer(): void {
  clearDeltaBuffer(contentBuffer)
}

export function clearToolArgsDeltaBuffer(): void {
  clearToolDeltaBuffer(toolArgsBuffer)
}

/**
 * Drop pending batched args for one tool call without applying them.
 * Needed when a later `tool_call_start` carries the authoritative full
 * `arguments` string — otherwise a deferred flush would append chunks that
 * were already included in that snapshot (duplicate / trailing junk).
 */
export function clearToolArgsDeltaBufferForTool(
  messageId: string,
  toolCallId: string,
  traceId?: string,
  scopedMessageId?: string
): void {
  const key = toolBufferKey(messageId, toolCallId, traceId, scopedMessageId)
  toolArgsPending.delete(key)
}

export function clearToolOutputDeltaBuffer(): void {
  clearToolDeltaBuffer(toolOutputBuffer)
}

export function clearWebSearchOutputDeltaBuffer(): void {
  clearToolDeltaBuffer(webSearchOutputBuffer)
}

export function clearAssistantJsonPartialBuffer(): void {
  if (jsonPartialBuffer.flushTimer != null) clearTimeout(jsonPartialBuffer.flushTimer)
  jsonPartialBuffer.flushTimer = undefined
  jsonPartialBuffer.pending.clear()
}

export function clearStreamDeltaBuffers(): void {
  clearReasoningDeltaBuffer()
  clearContentDeltaBuffer()
  clearToolArgsDeltaBuffer()
  clearToolOutputDeltaBuffer()
  clearWebSearchOutputDeltaBuffer()
  clearAssistantJsonPartialBuffer()
}
