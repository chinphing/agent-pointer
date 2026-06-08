export const CHANNEL_LABELS: Record<string, string> = {
  weixin: '微信',
  feishu: '飞书',
  wecom: '企微',
  dingtalk: '钉钉'
}

export function channelLabel(channel: string): string {
  return CHANNEL_LABELS[channel] ?? channel
}
