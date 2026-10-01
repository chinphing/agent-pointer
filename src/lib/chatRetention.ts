import type { ChatMessage } from '../types/chat'

/**
 * Retained chat text, measured by counting characters rather than bytes.
 *
 * Why this exists: the macOS WKWebView build exposes no JS heap
 * (`performance.memory` is Chromium-only), so the only readable signal for "is the
 * transcript what is growing, or is something leaking?" is how much text the
 * messages still hold. The transcript *is* the dominant holder in a long
 * conversation, so it has to be measurable to be told apart from a leak.
 *
 * What counts as retained, per message: `content`, `reasoning`, `thoughts`, and
 * each tool call's `arguments` and `result`. Those are the fields the chat store
 * can clear independently — `asideEvicted` clears `reasoning`/`thoughts`,
 * `bodyEvicted` clears a tool call's `arguments`/`result` — so the sum falls when
 * the store evicts and rises when a turn streams in.
 *
 * Cost: one pass over the messages and their tool calls, no allocation per
 * message, no copies. Callers must not run it on a hot tick (see
 * `lib/residencyProbe.ts`, which throttles it to one read every few seconds).
 */

export interface ChatRetention {
  /** Lead transcript messages walked. */
  leadMessages: number
  /** Characters the lead transcript retains. */
  leadChars: number
  /** Live scoped rows walked. */
  scopedRows: number
  /** Characters those scoped rows retain. */
  scopedChars: number
  /** Tool calls still holding body text (`arguments` / `result`), lead + scoped. */
  toolBodies: number
  /** Messages still holding a `reasoning` / `thoughts` aside, lead + scoped. */
  asides: number
}

/** Shared zero reading — frozen so a consumer cannot corrupt it. */
export const EMPTY_CHAT_RETENTION: ChatRetention = Object.freeze({
  leadMessages: 0,
  leadChars: 0,
  scopedRows: 0,
  scopedChars: 0,
  toolBodies: 0,
  asides: 0
})

interface RetentionWalk {
  messages: number
  chars: number
  toolBodies: number
  asides: number
}

/**
 * Every field is read defensively (`?.length ?? 0`): a message that came off the
 * wire may be missing `reasoning`, `thoughts` or `toolCalls` entirely, and a
 * missing field must count as nothing rather than as `NaN` or as a dropped message.
 */
function walkMessages(messages: readonly ChatMessage[]): RetentionWalk {
  let chars = 0
  let toolBodies = 0
  let asides = 0

  for (const message of messages) {
    chars += message.content?.length ?? 0

    const reasoning = message.reasoning?.length ?? 0
    const thoughts = message.thoughts?.length ?? 0
    chars += reasoning + thoughts
    if (message.asideEvicted !== true && reasoning + thoughts > 0) asides += 1

    for (const toolCall of message.toolCalls ?? []) {
      const body = (toolCall.arguments?.length ?? 0) + (toolCall.result?.length ?? 0)
      chars += body
      if (toolCall.bodyEvicted !== true && body > 0) toolBodies += 1
    }
  }

  return { messages: messages.length, chars, toolBodies, asides }
}

/** Retained text of the lead transcript and of the scoped rows, in one pass each. */
export function measureChatRetention(
  leadMessages: readonly ChatMessage[],
  scopedRows: readonly ChatMessage[]
): ChatRetention {
  const lead = walkMessages(leadMessages)
  const scoped = walkMessages(scopedRows)
  return {
    leadMessages: lead.messages,
    leadChars: lead.chars,
    scopedRows: scoped.messages,
    scopedChars: scoped.chars,
    toolBodies: lead.toolBodies + scoped.toolBodies,
    asides: lead.asides + scoped.asides
  }
}
