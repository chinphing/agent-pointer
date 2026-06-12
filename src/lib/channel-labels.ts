export const CHANNEL_LABELS: Record<string, string> = {
  weixin: '微信',
  feishu: '飞书',
  wecom: '企微',
  dingtalk: '钉钉'
}

const IM_CHANNELS = new Set(Object.keys(CHANNEL_LABELS))

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
  return CHANNEL_LABELS[channel] ?? channel
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
    const titled = `${label} · ${senderName}`
    return epoch > 0 ? `${titled} · 新对话` : titled
  }
  const firstUserText = opts?.firstUserText?.trim()
  if (firstUserText) return `${label} · ${firstUserText.slice(0, 24)}`
  if (epoch > 0) return `${label} · 新对话`
  if (baseId.includes(':group:')) return `${label} 群聊`
  return `${label} 私信`
}
