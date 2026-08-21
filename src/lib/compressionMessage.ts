import type { ChatMessage, ContextCompressionInfo } from '../types/chat'

export const COMPRESSION_SUMMARY_PREFIX = '[Conversation summary (auto-compression'

/** User-row summary injected after context compression. */
export function isCompressionSummaryMessage(message: ChatMessage): boolean {
  return (
    message.role === 'user' &&
    message.content.trimStart().startsWith(COMPRESSION_SUMMARY_PREFIX)
  )
}

/** Strip the auto-compression header from a summary user row for display. */
export function compressionSummaryBody(content: string): string {
  const lines = content.split('\n')
  if (lines.length <= 1) return content.trim()
  const body = lines.slice(1).join('\n').trim()
  return body || content.trim()
}

export function buildCompressionNoticeContent(info: ContextCompressionInfo): string {
  const dropped = info.droppedCount
  if (info.scope === 'sub_agent') {
    const name = info.subAgentName?.trim() || '子 Agent'
    return `【压缩】${name} 子任务内已将较早 ${dropped} 条记录摘要为 1 条（主对话不变）。`
  }
  if (info.reason === 'tool_limit' || info.reason === 'tool_limit_in_run') {
    return `【压缩】工具轮次触发：已将较早 ${dropped} 条对话摘要为 1 条，并保留最近对话原文。`
  }
  if (info.reason === 'in_run' || info.reason === 'overflow_in_run' || info.reason === 'in_run_drop') {
    return `【压缩】已将当前轮次 ${dropped} 条过程摘要为 1 条，并保留你的消息与最近原文。`
  }
  return `【压缩】已将较早 ${dropped} 条对话摘要为 1 条，并保留最近对话原文。`
}

/** In-thread tool-row label while compression LLM is running. */
export function buildCompressionProgressLabel(info: {
  scope?: string
  subAgentName?: string | null
}): string {
  if (info.scope === 'sub_agent') {
    const name = info.subAgentName?.trim() || '子 Agent'
    return `${name} 子任务内正在压缩较早记录`
  }
  return '正在压缩较早记录'
}
