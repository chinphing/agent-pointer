import { t } from '../i18n'

/** IM channel ids used for conversation-id detection. */
const IM_CHANNELS = new Set(['weixin', 'feishu', 'wecom', 'dingtalk'])

/** Brand display names from settings.channels.tabs (locale-aware). */
export function channelLabels(): Record<string, string> {
  return {
    weixin: t('settings.channels.tabs.weixin'),
    feishu: t('settings.channels.tabs.feishu'),
    wecom: t('settings.channels.tabs.wecom'),
    dingtalk: t('settings.channels.tabs.dingtalk')
  }
}

/** @deprecated Prefer channelLabels() so labels follow the active locale. */
export const CHANNEL_LABELS: Record<string, string> = channelLabels()

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

export function channelLabel(channel: string): string {
  return channelLabels()[channel] ?? channel
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
