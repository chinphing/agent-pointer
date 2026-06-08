export interface ChannelAccountConfig {
  enabled?: boolean
  name?: string
  connectionMode?: string
  appId?: string
  appSecret?: string
  encryptKey?: string
  verificationToken?: string
  clientId?: string
  clientSecret?: string
  corpId?: string
  agentId?: string
  secret?: string
  token?: string
  encodingAesKey?: string
  botId?: string
  websocketUrl?: string
  dmPolicy?: string
  groupPolicy?: string
  requireMention?: boolean
  allowFrom?: string[]
  groupAllowFrom?: string[]
}

export interface ChannelsMeta {
  publicBaseUrl?: string
}

export interface ChannelsConfig {
  meta?: ChannelsMeta
  feishu?: Record<string, ChannelAccountConfig>
  dingtalk?: Record<string, ChannelAccountConfig>
  wecom?: Record<string, ChannelAccountConfig>
  weixin?: Record<string, ChannelAccountConfig>
}

export interface ChannelStatusItem {
  channel: string
  accountId: string
  enabled: boolean
  connected: boolean
  webhookUrl: string
}

export interface ChannelsStatusResponse {
  channels: ChannelStatusItem[]
}

export interface WeixinQrLoginSession {
  accountId: string
  qrcode: string
  qrcodePngBase64: string
  status: string
}

export interface ChannelRegistrationSession {
  channel: string
  accountId: string
  qrUrl: string
  qrcodePngBase64: string
  status: string
  appId?: string
  appSecret?: string
  clientId?: string
  clientSecret?: string
  errorMessage?: string
}
