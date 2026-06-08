import { invoke } from '@tauri-apps/api/core'
import type {
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
  code: string
  senderId: string
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
  return res.pending.map(([code, senderId]) => ({ code, senderId }))
}
