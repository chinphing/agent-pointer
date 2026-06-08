import { invoke } from '@tauri-apps/api/core'
import type {
  ChannelRegistrationSession,
  ChannelsConfig,
  ChannelsStatusResponse,
  WeixinQrLoginSession
} from '../types/channels'
import { isTauriRuntime, WEB_API_BASE } from './runtime'

const TIMEOUT_MS = 12_000

async function webRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  if (init?.body && !(init.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json')
  }
  const controller = new AbortController()
  const timeoutId = window.setTimeout(() => controller.abort(), TIMEOUT_MS)
  try {
    const res = await fetch(`${WEB_API_BASE}${path}`, {
      ...init,
      headers,
      signal: init?.signal ?? controller.signal
    })
    if (!res.ok) {
      const text = await res.text()
      throw new Error(text || `请求失败 (${res.status})`)
    }
    if (res.status === 204 || res.status === 202) return undefined as T
    return await res.json()
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error('请求超时，请确认 pointer-server 已启动')
    }
    if (e instanceof TypeError) {
      throw new Error(
        `无法连接 pointer-server（${WEB_API_BASE}）。请先运行 npm run server:dev，或使用 Tauri 桌面端本地保存配置。`
      )
    }
    throw e
  } finally {
    window.clearTimeout(timeoutId)
  }
}

export async function getChannelsConfig(): Promise<ChannelsConfig> {
  if (isTauriRuntime()) {
    return invoke<ChannelsConfig>('get_channels_config')
  }
  return webRequest<ChannelsConfig>('/api/channels/config')
}

export async function listChannelStatus(): Promise<ChannelsStatusResponse> {
  if (isTauriRuntime()) {
    return invoke<ChannelsStatusResponse>('list_channel_status')
  }
  return webRequest<ChannelsStatusResponse>('/api/channels')
}

export async function updateChannelsConfig(cfg: ChannelsConfig): Promise<void> {
  if (isTauriRuntime()) {
    await invoke('update_channels_config', { cfg })
    return
  }
  await webRequest('/api/channels', { method: 'PUT', body: JSON.stringify(cfg) })
}

export async function getChannelWebhookUrl(
  channel: string,
  accountId: string
): Promise<string> {
  if (isTauriRuntime()) {
    return invoke<string>('get_channel_webhook_url', { channel, accountId })
  }
  const j = await webRequest<{ webhookUrl: string }>(
    `/api/channels/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}/webhook-url`
  )
  return j.webhookUrl
}

export async function startWeixinLogin(accountId: string): Promise<WeixinQrLoginSession> {
  if (isTauriRuntime()) {
    return invoke<WeixinQrLoginSession>('start_weixin_login', { accountId })
  }
  return webRequest(`/api/channels/weixin/${encodeURIComponent(accountId)}/login/start`, {
    method: 'POST'
  })
}

export async function getWeixinLoginStatus(
  accountId: string
): Promise<WeixinQrLoginSession | null> {
  if (isTauriRuntime()) {
    const session = await invoke<WeixinQrLoginSession | null>('get_weixin_login_status', {
      accountId
    })
    return session
  }
  return webRequest(`/api/channels/weixin/${encodeURIComponent(accountId)}/login/status`)
}

export async function startChannelRegistration(
  channel: string,
  accountId: string
): Promise<ChannelRegistrationSession> {
  if (isTauriRuntime()) {
    return invoke<ChannelRegistrationSession>('start_channel_registration', {
      channel,
      accountId
    })
  }
  return webRequest<ChannelRegistrationSession>(
    `/api/channels/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}/register/start`,
    { method: 'POST' }
  )
}

export async function getChannelRegistrationStatus(
  channel: string,
  accountId: string
): Promise<ChannelRegistrationSession | null> {
  if (isTauriRuntime()) {
    return invoke<ChannelRegistrationSession | null>('get_channel_registration_status', {
      channel,
      accountId
    })
  }
  return webRequest<ChannelRegistrationSession | null>(
    `/api/channels/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}/register/status`
  )
}

export async function hasWeixinCredentials(accountId: string): Promise<boolean> {
  if (isTauriRuntime()) {
    return invoke<boolean>('has_weixin_credentials', { accountId })
  }
  return false
}

export async function approveChannelPairing(
  channel: string,
  accountId: string,
  code: string
): Promise<void> {
  if (isTauriRuntime()) {
    await invoke('approve_channel_pairing', { channel, accountId, code })
    return
  }
  await webRequest(
    `/api/channels/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}/pairing/approve`,
    { method: 'POST', body: JSON.stringify({ code }) }
  )
}

export interface PairingPendingItem {
  channel: string
  code: string
  senderId: string
}

const PAIRING_CHANNELS = ['weixin', 'wecom', 'feishu', 'dingtalk'] as const

export async function listAllChannelPairingPending(
  accountId: string
): Promise<PairingPendingItem[]> {
  const all: PairingPendingItem[] = []
  for (const channel of PAIRING_CHANNELS) {
    const pending = await listChannelPairingPending(channel, accountId)
    for (const item of pending) {
      all.push({ ...item, channel })
    }
  }
  return all
}

export async function approveChannelPairingAny(
  accountId: string,
  code: string
): Promise<string> {
  let lastError = '配对码无效或已过期'
  for (const channel of PAIRING_CHANNELS) {
    try {
      await approveChannelPairing(channel, accountId, code)
      return channel
    } catch (e) {
      lastError = e instanceof Error ? e.message : String(e)
    }
  }
  throw new Error(lastError)
}

export async function listChannelPairingPending(
  channel: string,
  accountId: string
): Promise<PairingPendingItem[]> {
  if (isTauriRuntime()) {
    const res = await invoke<{ pending: PairingPendingItem[] }>(
      'list_channel_pairing_pending',
      { channel, accountId }
    )
    return res.pending
  }
  const res = await webRequest<{ pending: [string, string][] }>(
    `/api/channels/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}/pairing/pending`
  )
  return res.pending.map(([code, senderId]) => ({ channel, code, senderId }))
}
