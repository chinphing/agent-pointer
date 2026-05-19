import type { ChatMessage } from '../types/chat'

export const TASK_BOARD_TRIM_PREFIX = '[History trimmed after task_board update]'

export function isTaskBoardTrimMessage(message: ChatMessage): boolean {
  return (
    message.role === 'user' &&
    message.content.trimStart().startsWith(TASK_BOARD_TRIM_PREFIX)
  )
}

export function taskBoardTrimNoticeBody(content: string): string {
  const lines = content.split('\n')
  if (lines.length <= 1) return content.trim()
  const body = lines.slice(1).join('\n').trim()
  return body || content.trim()
}
