export const CHANNEL_LABELS: Record<string, string> = {
  weixin: '微信',
  feishu: '飞书',
  wecom: '企微',
  dingtalk: '钉钉'
}

const IM_CHANNELS = new Set(Object.keys(CHANNEL_LABELS))

export function channelLabel(channel: string): string {
  return CHANNEL_LABELS[channel] ?? channel
}

export function isImConversation(conversationId: string): boolean {
  const channel = conversationId.split(':')[0] ?? ''
  return IM_CHANNELS.has(channel)
}

export function imConversationTitle(
  conversationId: string,
  opts?: { senderName?: string; firstUserText?: string }
): string {
  const channel = conversationId.split(':')[0] ?? ''
  const label = channelLabel(channel)
  const senderName = opts?.senderName?.trim()
  if (senderName) return `${label} · ${senderName}`
  const firstUserText = opts?.firstUserText?.trim()
  if (firstUserText) return `${label} · ${firstUserText.slice(0, 24)}`
  if (conversationId.includes(':group:')) return `${label} 群聊`
  return `${label} 私信`
}
