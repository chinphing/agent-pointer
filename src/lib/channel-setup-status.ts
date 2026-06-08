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

const SETUP_LABELS: Record<SetupState, string> = {
  no_credentials: '待扫码配置',
  not_enabled: '凭证已配置，点击连接',
  webhook_mode: '当前为 Webhook 模式，请改回长连接后连接',
  ready: '已配置，点击连接'
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
  if (
    text.includes('成功') ||
    text.includes('凭证已填入') ||
    text.includes('创建成功')
  ) {
    return 'success'
  }
  if (
    text.includes('失败') ||
    text.includes('过期') ||
    text.includes('超时') ||
    text.includes('拒绝') ||
    text.includes('取消') ||
    text.includes('拦截')
  ) {
    return 'error'
  }
  if (text.includes('已扫码') || text.includes('等待')) {
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
  if (connected === true) return '运行中，可正常收发消息'
  if (connected === false) return '未连接，请点击「连接」'
  return SETUP_LABELS.ready
}

export function resolveChannelStatus(
  tab: ChannelTab,
  cfg: ChannelsConfig,
  weixinLoggedIn: boolean,
  sessionText: string,
  connected?: boolean
): ChannelStatusView {
  const setup = resolveSetupState(tab, cfg, weixinLoggedIn)
  let setupLabel = SETUP_LABELS[setup]
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
