import type { ChatMessage, ToolCall } from '../types/chat'

export type CurrentConversationSearchMatch = {
  messageId: string
  toolCallId?: string
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

/** Body and tool-call matches in transcript and tool-call order. */
export function findCurrentConversationMatches(
  messages: ChatMessage[],
  query: string
): CurrentConversationSearchMatch[] {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return []

  return messages.flatMap(message => {
    const matches: CurrentConversationSearchMatch[] = []
    const bodyText = searchableValues([
      message.content,
      message.reasoning,
      message.errorMessage
    ])
    if (bodyText.toLocaleLowerCase().includes(needle)) {
      matches.push({ messageId: message.id })
    }
    for (const tool of message.toolCalls ?? []) {
      if (searchableToolCallText(tool).toLocaleLowerCase().includes(needle)) {
        matches.push({ messageId: message.id, toolCallId: tool.id })
      }
    }
    return matches
  })
}
