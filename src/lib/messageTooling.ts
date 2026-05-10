import type { ToolCall } from '../types/chat'

export function toolCallBaseName(name: string): string {
  const i = name.indexOf(':')
  return i === -1 ? name : name.slice(0, i)
}

/** `response` tool is not shown as a card (matches backend). */
export function visibleToolCalls(toolCalls: ToolCall[] | undefined): ToolCall[] {
  return toolCalls?.filter(tc => toolCallBaseName(tc.name) !== 'response') ?? []
}
