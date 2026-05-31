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

export const CHARS_PER_THINKING_DOT = 100
export const MAX_THINKING_DOTS = 48

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
  const segments = n <= 0 ? 1 : Math.ceil(n / CHARS_PER_THINKING_DOT)
  return Math.min(MAX_THINKING_DOTS, segments)
}

export function thinkingLabel(streamedCharCount: number): string {
  return `思考中${'.'.repeat(thinkingDotCount(streamedCharCount))}`
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
