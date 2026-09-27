import { t } from '../i18n'

const IM_CHANNEL_IDS = ['weixin', 'feishu', 'wecom', 'dingtalk'] as const

/** Channel id → short UI label (live via vue-i18n). */
export function channelLabel(channel: string): string {
  if ((IM_CHANNEL_IDS as readonly string[]).includes(channel)) {
    return t(`settings.channels.tabs.${channel}`)
  }
  return channel
}

/** Snapshot of channel labels for callers that need a Record (re-resolve each call). */
export function channelLabels(): Record<string, string> {
  return Object.fromEntries(IM_CHANNEL_IDS.map(id => [id, channelLabel(id)]))
}

/** @deprecated Prefer channelLabel(); kept for IM_CHANNELS membership checks. */
export const CHANNEL_LABELS: Record<string, string> = Object.fromEntries(
  IM_CHANNEL_IDS.map(id => [id, id])
)

const IM_CHANNELS = new Set<string>(IM_CHANNEL_IDS)

export function imBaseConversationId(conversationId: string): string {
  const at = conversationId.lastIndexOf('@s')
  if (at > 0) {
    const rest = conversationId.slice(at + 2)
    if (/^\d+$/.test(rest)) return conversationId.slice(0, at)
  }
  return conversationId
}

export function imSessionEpoch(conversationId: string): number {
  const at = conversationId.lastIndexOf('@s')
  if (at > 0) {
    const rest = conversationId.slice(at + 2)
    if (/^\d+$/.test(rest)) return Number(rest)
  }
  return 0
}

export function isImConversation(conversationId: string): boolean {
  const channel = imBaseConversationId(conversationId).split(':')[0] ?? ''
  return IM_CHANNELS.has(channel)
}

export function imConversationTitle(
  conversationId: string,
  opts?: { senderName?: string; firstUserText?: string }
): string {
  const baseId = imBaseConversationId(conversationId)
  const channel = baseId.split(':')[0] ?? ''
  const label = channelLabel(channel)
  const epoch = imSessionEpoch(conversationId)
  const senderName = opts?.senderName?.trim()
  if (senderName) {
    return epoch > 0
      ? t('channel.conversation.newChatWithSender', { label, sender: senderName })
      : `${label} · ${senderName}`
  }
  const firstUserText = opts?.firstUserText?.trim()
  if (firstUserText) return `${label} · ${firstUserText.slice(0, 24)}`
  if (epoch > 0) return t('channel.conversation.newChat', { label })
  if (baseId.includes(':group:')) return t('channel.conversation.group', { label })
  return t('channel.conversation.dm', { label })
}
