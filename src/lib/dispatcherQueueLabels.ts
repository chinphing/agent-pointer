/** Human-readable labels for dispatcher lane queue UI. */

export function laneQueueLabel(lane: string): string {
  if (lane === 'global:main') return '主任务池'
  if (lane === 'global:cron') return '定时任务池'
  if (lane.startsWith('session:')) {
    const id = lane.slice('session:'.length)
    return `会话 ${shortId(id)}`
  }
  return lane
}

export function triggerSourceLabel(source: string): string {
  switch (source) {
    case 'ipc':
      return '聊天'
    case 'http_runs':
      return 'HTTP'
    case 'webhook':
      return 'Webhook'
    case 'cron':
      return '定时'
    case 'im':
      return 'IM'
    case 'internal':
      return '内部'
    default:
      return source
  }
}

export function shortId(id: string, max = 20): string {
  const t = id.trim()
  if (t.length <= max) return t
  return `${t.slice(0, max - 1)}…`
}
