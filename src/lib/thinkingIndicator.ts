import type { ChatMessage, ToolCall } from '../types/chat'

export interface ThinkingStreamBody {
  content?: string
  rawContent?: string
  thoughts?: string
  toolNamePreview?: string
  responseTextDraft?: string
  reasoning?: string
  toolCalls?: ToolCall[]
}

/** First-tier: each of the first 10 dots covers this many streamed chars. */
export const CHARS_PER_THINKING_DOT = 100
/** After every this many dots, chars-per-dot doubles. */
export const THINKING_DOTS_PER_TIER = 10
export const MAX_THINKING_DOTS = 48
/** Trailing dots that keep waving after the count hits the cap. */
export const THINKING_LIVE_DOT_COUNT = 3

/** Cumulative streamed chars covered by `dotCount` dots (capped at max). */
export function charsCoveredByThinkingDots(dotCount: number): number {
  const dots = Math.max(0, Math.min(MAX_THINKING_DOTS, Math.floor(dotCount)))
  let remaining = dots
  let perDot = CHARS_PER_THINKING_DOT
  let total = 0
  while (remaining > 0) {
    const inTier = Math.min(THINKING_DOTS_PER_TIER, remaining)
    total += inTier * perDot
    remaining -= inTier
    perDot *= 2
  }
  return total
}

/** 48 dots: 10×100 + 10×200 + 10×400 + 10×800 + 8×1600 = 27800. */
export const MAX_THINKING_DOT_CHARS = charsCoveredByThinkingDots(MAX_THINKING_DOTS)

export function streamedCharCountFromBody(body: ThinkingStreamBody): number {
  const c = body.content?.length ?? 0
  const raw = body.rawContent?.length ?? 0
  const thoughtsLen = body.thoughts?.length ?? 0
  const toolPreview = body.toolNamePreview?.length ?? 0
  const draftLen = body.responseTextDraft?.length ?? 0
  const reasoningLen = body.reasoning?.length ?? 0
  return Math.max(c, raw, thoughtsLen, toolPreview, draftLen, reasoningLen)
}

export function streamedCharCountFromMessage(message: ChatMessage): number {
  return streamedCharCountFromBody({
    content: message.content,
    rawContent: message.rawContent,
    thoughts: message.thoughts,
    toolNamePreview: message.toolNamePreview,
    responseTextDraft: message.responseTextDraft,
    reasoning: message.reasoning
  })
}

export function thinkingDotCount(streamedCharCount: number): number {
  const n = streamedCharCount
  if (n <= 0) return 1
  if (n > MAX_THINKING_DOT_CHARS) return MAX_THINKING_DOTS

  let remaining = n
  let perDot = CHARS_PER_THINKING_DOT
  let dots = 0
  while (dots < MAX_THINKING_DOTS && remaining > 0) {
    const tierCap = Math.min(THINKING_DOTS_PER_TIER, MAX_THINKING_DOTS - dots)
    const need = Math.min(tierCap, Math.ceil(remaining / perDot))
    dots += need
    remaining -= need * perDot
    perDot *= 2
  }
  return dots
}

export function thinkingDotsAtCap(streamedCharCount: number): boolean {
  return thinkingDotCount(streamedCharCount) >= MAX_THINKING_DOTS
}

/** Dots that stay still; live trailing dots are rendered separately when capped. */
export function thinkingSteadyDotCount(streamedCharCount: number): number {
  const dots = thinkingDotCount(streamedCharCount)
  if (dots >= MAX_THINKING_DOTS) return MAX_THINKING_DOTS - THINKING_LIVE_DOT_COUNT
  return dots
}

export function thinkingLabel(streamedCharCount: number): string {
  return `思考中${'.'.repeat(thinkingDotCount(streamedCharCount))}`
}

function isStreamingAssistant(msg: ChatMessage): boolean {
  return msg.status === 'streaming' || msg.contentStreaming === true
}

/**
 * Collapsed sub-agent「思考中」count.
 * Do not use `latestStreamBody`: that merge copies the previous reply into
 * `content`, so max() freezes the dots until thoughts exceed the old reply.
 * Only this streaming row (and the live session if writes landed there).
 */
export function thinkingCharCountForCollapsedSubAgent(
  spawnRows: readonly ChatMessage[],
  liveSession?: { thoughts?: string; reasoning?: string; contentStreaming?: boolean } | null
): number {
  const assistants = spawnRows.filter(m => m.role === 'assistant')
  const streaming = [...assistants].reverse().find(isStreamingAssistant)
  const row = streaming ?? assistants[assistants.length - 1]
  let n = row ? streamedCharCountFromMessage(row) : 0
  if (liveSession?.contentStreaming === true) {
    n = Math.max(
      n,
      liveSession.thoughts?.length ?? 0,
      liveSession.reasoning?.length ?? 0
    )
  }
  return n
}

/** True when the row already shows text, tools, or other visible streaming output. */
export function messageHasVisibleStreamingActivity(message: ChatMessage): boolean {
  if (message.content?.trim()) return true
  if (message.thoughts?.trim()) return true
  if (message.reasoning?.trim()) return true
  if (message.toolNamePreview?.trim()) return true
  if (message.responseTextDraft?.trim()) return true
  if ((message.toolCalls?.length ?? 0) > 0) return true
  return false
}

export function bodyHasVisibleStreamingActivity(body: ThinkingStreamBody): boolean {
  if (body.content?.trim()) return true
  if (body.thoughts?.trim()) return true
  if (body.reasoning?.trim()) return true
  if (body.toolNamePreview?.trim()) return true
  if (body.responseTextDraft?.trim()) return true
  if ((body.toolCalls?.length ?? 0) > 0) return true
  return false
}

/**
 * Reply text / tools the user can already see.
 * Hidden `thoughts` / `reasoning` only drive the dot count — they must not
 * keep「思考中」once content or a tool row is on screen.
 */
export function bodyHasVisibleUserFacingOutput(body: ThinkingStreamBody): boolean {
  if (bodyHasVisibleReplyText(body)) return true
  if ((body.toolCalls?.length ?? 0) > 0) return true
  return false
}

/** Reply text only — process tools on the host must not keep「思考中」stuck. */
export function bodyHasVisibleReplyText(body: ThinkingStreamBody): boolean {
  if (body.content?.trim()) return true
  if (body.toolNamePreview?.trim()) return true
  if (body.responseTextDraft?.trim()) return true
  return false
}

export function toolCallStatusInProgress(status: string | undefined): boolean {
  return status === 'running' || status === 'pending' || status === 'pending_approval'
}

/** Collapsed tool header stays live for in-flight tools or a thinking gap — not after reply text. */
export function collapsedProcessRunActive(args: {
  generating: boolean
  isActiveHost: boolean
  host: ThinkingStreamBody
  toolCalls: { status?: string }[]
}): boolean {
  if (args.toolCalls.some(tc => toolCallStatusInProgress(tc.status))) return true
  if (bodyHasVisibleReplyText(args.host)) return false
  return args.generating && args.isActiveHost
}

/**
 * Sub-agent collapsed header: show「思考中」while the task is running and
 * no inner tool is in flight. Do not require an empty latest tool list —
 * finished tools belong on the summary. Inner reply text is hidden when
 * collapsed, so it must not suppress the thinking line.
 */
export function subAgentThinkingActive(args: {
  running: boolean
  hasInProgressTool: boolean
}): boolean {
  return args.running && !args.hasInProgressTool
}

export function shouldShowThinkingIndicator(args: {
  runInProgress: boolean
  markdownBodyVisible?: boolean
  thoughtsPanelVisible?: boolean
  reasoningVisible?: boolean
  extraVisibleTools?: boolean
  body: ThinkingStreamBody
}): boolean {
  if (!args.runInProgress) return false
  if (args.markdownBodyVisible) return false
  if (args.thoughtsPanelVisible) return false
  if (args.reasoningVisible) return false
  if (args.extraVisibleTools) return false
  if (bodyHasVisibleUserFacingOutput(args.body)) return false
  return true
}
