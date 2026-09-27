import { t } from '../i18n'
/** Human-readable labels for dispatcher lane queue UI. */

export function laneQueueLabel(lane: string): string {
  if (lane === 'global:main') return t('dispatcher.lane.main')
  if (lane === 'global:cron') return t('dispatcher.lane.cron')
  if (lane.startsWith('session:')) {
    const id = lane.slice('session:'.length)
    return t('dispatcher.lane.session', { id: shortId(id) })
  }
  return lane
}

export function triggerSourceLabel(source: string): string {
  switch (source) {
    case 'ipc':
      return t('dispatcher.kind.chat')
    case 'http_runs':
      return 'HTTP'
    case 'webhook':
      return 'Webhook'
    case 'cron':
      return t('dispatcher.kind.cron')
    case 'im':
      return 'IM'
    case 'internal':
      return t('dispatcher.kind.internal')
    default:
      return source
  }
}

export function shortId(id: string, max = 20): string {
  const trimmed = id.trim()
  if (trimmed.length <= max) return trimmed
  return `${trimmed.slice(0, max - 1)}…`
}
