import type { ChatMessage, ToolCall } from '../types/chat'

export type CurrentConversationSearchMatch = {
  messageId: string
  toolCallId?: string
  /**
   * Scoped sub-agent row whose content matched. `messageId` is then that row's anchor
   * (the row the frame hangs under), so navigation mounts the right transcript entry.
   */
  contentMessageId?: string
}

function searchableValues(values: unknown[]): string {
  return values
    .filter((value): value is string => typeof value === 'string' && value.length > 0)
    .join('\n')
}

/** Text exposed by one tool call in the current-conversation search surface. */
export function searchableToolCallText(tool: ToolCall): string {
  return searchableValues([
    tool.displayLabel,
    tool.displaySummary,
    tool.arguments,
    tool.result,
    tool.error,
    tool.terminalOutput,
    tool.webSearchOutput
  ])
}

/** Text exposed by one message in the current-conversation search surface. */
export function searchableMessageText(message: ChatMessage): string {
  return searchableValues([
    message.content,
    message.reasoning,
    message.errorMessage,
    ...(message.toolCalls ?? []).map(searchableToolCallText)
  ])
}

/** Body text only — the part that maps to a message (not a tool row) hit. */
function searchableBodyText(message: ChatMessage): string {
  return searchableValues([
    message.content,
    message.reasoning,
    message.errorMessage
  ])
}

function scopedRowOrder(row: ChatMessage): number {
  return row.position ?? row.createdAt
}

/**
 * Body and tool-call matches in transcript and tool-call order.
 *
 * `scopedMessages` are the conversation's sub-agent rows (they are kept out of the lead
 * transcript). A hit inside a frame is reported against that frame's anchor message plus
 * the id the frame renders — `toolCallId` for a tool row, `contentMessageId` for a round's
 * content block.
 */
export function findCurrentConversationMatches(
  messages: ChatMessage[],
  query: string,
  scopedMessages: readonly ChatMessage[] = []
): CurrentConversationSearchMatch[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return []

  const transcript = messages.flatMap(message => {
    const matches: CurrentConversationSearchMatch[] = []
    if (searchableBodyText(message).toLocaleLowerCase().includes(needle)) {
      matches.push({ messageId: message.id })
    }
    for (const tool of message.toolCalls ?? []) {
      if (searchableToolCallText(tool).toLocaleLowerCase().includes(needle)) {
        matches.push({ messageId: message.id, toolCallId: tool.id })
      }
    }
    return matches
  })

  const scoped = [...scopedMessages]
    .filter(message => !!message.anchorMessageId?.trim())
    .sort((a, b) => scopedRowOrder(a) - scopedRowOrder(b) || a.id.localeCompare(b.id))
    .flatMap(row => {
      const anchorMessageId = row.anchorMessageId!.trim()
      const matches: CurrentConversationSearchMatch[] = []
      if (searchableBodyText(row).toLocaleLowerCase().includes(needle)) {
        matches.push({ messageId: anchorMessageId, contentMessageId: row.id })
      }
      for (const tool of row.toolCalls ?? []) {
        if (searchableToolCallText(tool).toLocaleLowerCase().includes(needle)) {
          matches.push({ messageId: anchorMessageId, toolCallId: tool.id })
        }
      }
      return matches
    })

  return [...transcript, ...scoped]
}
