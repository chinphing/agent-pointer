import { t } from '../i18n'
import type { ChannelAccountConfig, ChannelsConfig } from '../types/channels'

export type ChannelTab = 'weixin' | 'feishu' | 'wecom' | 'dingtalk'

/** 配置就绪态：能否按当前表单启动 monitor（与 gateway spawn 条件对齐） */
export type SetupState = 'no_credentials' | 'not_enabled' | 'webhook_mode' | 'ready'

export type StatusTone = 'success' | 'warn' | 'error' | 'muted'

export interface ChannelStatusView {
  setup: SetupState
  setupLabel: string
  tabDot: 'none' | 'warn' | 'ok'
  sessionText: string
  displayText: string
  tone: StatusTone
}

function setupLabels(): Record<SetupState, string> {
  return {
    no_credentials: t('settings.channels.setup.noCredentials'),
    not_enabled: t('settings.channels.setup.notEnabled'),
    webhook_mode: t('settings.channels.setup.webhookMode'),
    ready: t('settings.channels.setup.ready')
  }
}

const PLACEHOLDER_EXACT = new Set(['xxx', 'cli_xxx', 'test_encrypt_key', 'test', 'placeholder'])

/** 文档/测试用占位值，不算有效凭证 */
export function isPlaceholderCredential(value?: string): boolean {
  const v = value?.trim() ?? ''
  if (!v) return true
  const lower = v.toLowerCase()
  if (PLACEHOLDER_EXACT.has(lower)) return true
  if (lower.endsWith('_xxx')) return true
  return false
}

export function effectiveCredential(value?: string): string {
  return isPlaceholderCredential(value) ? '' : value!.trim()
}

function cleanField(value?: string): string {
  return effectiveCredential(value)
}

function isPlaceholderUrl(value?: string): boolean {
  const lower = value?.trim().toLowerCase() ?? ''
  return (
    !lower ||
    lower.includes('your-ngrok') ||
    lower.includes('example.com') ||
    lower.includes('pointer.example')
  )
}

export function sanitizeAccountConfig(acc: ChannelAccountConfig): ChannelAccountConfig {
  return {
    ...acc,
    appId: cleanField(acc.appId),
    appSecret: cleanField(acc.appSecret),
    encryptKey: cleanField(acc.encryptKey),
    verificationToken: cleanField(acc.verificationToken),
    clientId: cleanField(acc.clientId),
    clientSecret: cleanField(acc.clientSecret),
    corpId: cleanField(acc.corpId),
    agentId: cleanField(acc.agentId),
    secret: cleanField(acc.secret),
    token: cleanField(acc.token),
    encodingAesKey: cleanField(acc.encodingAesKey),
    botId: cleanField(acc.botId),
    websocketUrl: cleanField(acc.websocketUrl)
  }
}

export function sanitizeChannelsConfig(cfg: ChannelsConfig): ChannelsConfig {
  const sanitizeMap = (map?: Record<string, ChannelAccountConfig>) => {
    if (!map) return map
    const out: Record<string, ChannelAccountConfig> = {}
    for (const [id, acc] of Object.entries(map)) {
      out[id] = sanitizeAccountConfig(acc)
    }
    return out
  }
  return {
    ...cfg,
    meta: {
      ...cfg.meta,
      publicBaseUrl: isPlaceholderUrl(cfg.meta?.publicBaseUrl)
        ? ''
        : cfg.meta?.publicBaseUrl?.trim() ?? ''
    },
    feishu: sanitizeMap(cfg.feishu),
    dingtalk: sanitizeMap(cfg.dingtalk),
    wecom: sanitizeMap(cfg.wecom),
    weixin: sanitizeMap(cfg.weixin)
  }
}

function account(cfg: ChannelsConfig, tab: ChannelTab): ChannelAccountConfig | undefined {
  return cfg[tab]?.default
}

export function hasChannelCredentials(
  tab: ChannelTab,
  cfg: ChannelsConfig,
  weixinLoggedIn: boolean
): boolean {
  const acc = account(cfg, tab)
  if (!acc) return false
  switch (tab) {
    case 'weixin':
      return weixinLoggedIn
    case 'feishu':
      return Boolean(
        effectiveCredential(acc.appId) && effectiveCredential(acc.appSecret)
      )
    case 'wecom':
      if (acc.connectionMode === 'webhook') {
        return Boolean(
          effectiveCredential(acc.corpId) &&
            effectiveCredential(acc.agentId) &&
            effectiveCredential(acc.secret) &&
            effectiveCredential(acc.token) &&
            effectiveCredential(acc.encodingAesKey)
        )
      }
      return Boolean(effectiveCredential(acc.botId) && effectiveCredential(acc.secret))
    case 'dingtalk':
      return Boolean(
        effectiveCredential(acc.clientId) && effectiveCredential(acc.clientSecret)
      )
  }
}

/** 与 gateway spawn_*_monitors 启动条件一致 */
export function isChannelReady(
  tab: ChannelTab,
  cfg: ChannelsConfig,
  weixinLoggedIn: boolean
): boolean {
  const acc = account(cfg, tab)
  if (!acc?.enabled) return false
  if (!hasChannelCredentials(tab, cfg, weixinLoggedIn)) return false
  if (tab === 'weixin') return true
  return normalizeConnectionMode(acc.connectionMode) === 'websocket'
}

export function normalizeConnectionMode(mode?: string): string {
  return mode === 'webhook' ? 'webhook' : 'websocket'
}

/** 首选 websocket；仅企微 Agent 专用 webhook 凭证齐全时保留 webhook */
export function preferConnectionMode(
  channel: 'feishu' | 'dingtalk' | 'wecom',
  acc: ChannelAccountConfig
): 'websocket' | 'webhook' {
  const saved = normalizeConnectionMode(acc.connectionMode)
  if (saved !== 'webhook') {
    return 'websocket'
  }
  if (channel === 'feishu' || channel === 'dingtalk') {
    return 'websocket'
  }
  const wsReady = Boolean(acc.botId?.trim() && acc.secret?.trim())
  const webhookReady = Boolean(
    acc.corpId?.trim() &&
      acc.agentId?.trim() &&
      acc.token?.trim() &&
      acc.encodingAesKey?.trim()
  )
  if (webhookReady && !wsReady) {
    return 'webhook'
  }
  return 'websocket'
}

export function resolveSetupState(
  tab: ChannelTab,
  cfg: ChannelsConfig,
  weixinLoggedIn: boolean
): SetupState {
  if (!hasChannelCredentials(tab, cfg, weixinLoggedIn)) {
    return 'no_credentials'
  }
  const acc = account(cfg, tab)!
  if (!acc.enabled) {
    return 'not_enabled'
  }
  if (tab !== 'weixin' && normalizeConnectionMode(acc.connectionMode) === 'webhook') {
    return 'webhook_mode'
  }
  return 'ready'
}

function sessionTone(text: string): StatusTone {
  const lower = text.toLowerCase()
  if (
    text.includes('连接未建立') ||
    text.includes('连接失败') ||
    lower.includes('connection not established') ||
    lower.includes('connection failed') ||
    lower.includes('failed to connect')
  ) {
    return 'error'
  }
  if (
    text.includes('成功') ||
    text.includes('凭证已填入') ||
    text.includes('创建成功') ||
    text.includes('运行中') ||
    lower.includes('success') ||
    lower.includes('credentials filled') ||
    lower.includes('created successfully') ||
    lower.includes('running')
  ) {
    return 'success'
  }
  if (
    text.includes('失败') ||
    text.includes('过期') ||
    text.includes('超时') ||
    text.includes('拒绝') ||
    text.includes('取消') ||
    text.includes('拦截') ||
    lower.includes('failed') ||
    lower.includes('expired') ||
    lower.includes('timeout') ||
    lower.includes('timed out') ||
    lower.includes('denied') ||
    lower.includes('cancelled') ||
    lower.includes('canceled') ||
    lower.includes('blocked')
  ) {
    return 'error'
  }
  if (
    text.includes('已扫码') ||
    text.includes('等待') ||
    text.includes('正在连接') ||
    text.includes('正在获取') ||
    lower.includes('scanned') ||
    lower.includes('waiting') ||
    lower.includes('connecting') ||
    lower.includes('fetching')
  ) {
    return 'warn'
  }
  return 'muted'
}

function setupTone(state: SetupState): StatusTone {
  if (state === 'ready') return 'success'
  if (state === 'no_credentials') return 'muted'
  return 'warn'
}

function resolveReadyLabel(connected?: boolean): string {
  if (connected === true) return t('settings.channels.setup.readyConnected')
  if (connected === false) return t('settings.channels.setup.readyDisconnected')
  return setupLabels().ready
}

export function resolveChannelStatus(
  tab: ChannelTab,
  cfg: ChannelsConfig,
  weixinLoggedIn: boolean,
  sessionText: string,
  connected?: boolean
): ChannelStatusView {
  const setup = resolveSetupState(tab, cfg, weixinLoggedIn)
  let setupLabel = setupLabels()[setup]
  if (setup === 'ready') {
    setupLabel = resolveReadyLabel(connected)
  }
  const tabDot: ChannelStatusView['tabDot'] =
    setup === 'ready' && connected === true
      ? 'ok'
      : setup === 'ready'
        ? 'warn'
        : setup === 'no_credentials'
          ? 'none'
          : 'warn'

  const displayText = sessionText.trim() || setupLabel
  let tone = sessionText.trim() ? sessionTone(sessionText) : setupTone(setup)
  if (!sessionText.trim() && setup === 'ready' && connected === true) {
    tone = 'success'
  }
  if (!sessionText.trim() && setup === 'ready' && connected === false) {
    tone = 'warn'
  }

  return {
    setup,
    setupLabel,
    tabDot,
    sessionText: sessionText.trim(),
    displayText,
    tone
  }
}
