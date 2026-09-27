/** Human-readable labels for dispatcher lane queue UI. */

import { t } from '../i18n'

export function laneQueueLabel(lane: string): string {
  if (lane === 'global:main') return t('settings.queue.laneMain')
  if (lane === 'global:cron') return t('settings.queue.laneCron')
  if (lane.startsWith('session:')) {
    const id = lane.slice('session:'.length)
    return t('settings.queue.laneSession', { id: shortId(id) })
  }
  return lane
}

export function triggerSourceLabel(source: string): string {
  switch (source) {
    case 'ipc':
      return t('settings.queue.sourceChat')
    case 'http_runs':
      return 'HTTP'
    case 'webhook':
      return 'Webhook'
    case 'cron':
      return t('settings.queue.sourceCron')
    case 'im':
      return 'IM'
    case 'internal':
      return t('settings.queue.sourceInternal')
    default:
      return source
  }
}

export function shortId(id: string, max = 20): string {
  const tId = id.trim()
  if (tId.length <= max) return tId
  return `${tId.slice(0, max - 1)}…`
}
