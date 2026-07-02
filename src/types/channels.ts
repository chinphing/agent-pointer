export const DEFAULT_CHANNEL_IDLE_MINUTES = 60

export interface SessionResetConfig {
  idleMinutes?: number
}

export interface DynamicAgentsConfig {
  enabled?: boolean
  dmCreateAgent?: boolean
  groupEnabled?: boolean
  adminUsers?: string[]
}

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
  dynamicAgents?: DynamicAgentsConfig
}

export interface ImOutboundConfig {
  /** Push assistant text to IM after each model round. Default true. */
  sendIntermediateText?: boolean
  /** Push tool-call progress lines to IM during agent runs. Default true. */
  sendToolCalls?: boolean
}

export interface ChannelsMeta {
  publicBaseUrl?: string
  sessionReset?: SessionResetConfig
  imOutbound?: ImOutboundConfig
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
  botId?: string
  secret?: string
  errorMessage?: string
}
