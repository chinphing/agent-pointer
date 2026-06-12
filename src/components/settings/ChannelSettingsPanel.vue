<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ChevronDown, CircleHelp, Copy, Plug, QrCode, RefreshCw } from 'lucide-vue-next'
import {
  DEFAULT_CHANNEL_IDLE_MINUTES,
  type ChannelAccountConfig,
  type ChannelsConfig
} from '../../types/channels'
import {
  approveChannelPairingAny,
  getChannelRegistrationStatus,
  getChannelsConfig,
  getChannelWebhookUrl,
  getWeixinLoginStatus,
  hasWeixinCredentials,
  listChannelStatus,
  startChannelRegistration,
  startWeixinLogin,
  updateChannelsConfig
} from '../../lib/channels'
import { isTauriRuntime } from '../../lib/runtime'
import {
  normalizeConnectionMode,
  preferConnectionMode,
  resolveChannelStatus,
  sanitizeChannelsConfig,
  type ChannelTab
} from '../../lib/channel-setup-status'

const loading = ref(false)
const saving = ref(false)
const connecting = ref(false)
const error = ref('')
const pairingCode = ref('')
const pairingSuccess = ref('')

const CHANNEL_TABS: { id: ChannelTab; label: string }[] = [
  { id: 'weixin', label: '微信' },
  { id: 'feishu', label: '飞书' },
  { id: 'wecom', label: '企微' },
  { id: 'dingtalk', label: '钉钉' }
]

const activeTab = ref<ChannelTab>('weixin')

const weixinQr = ref('')
const weixinLoginStatus = ref('')
const weixinLoggedIn = ref(false)
const weixinBusy = ref(false)

const qrByChannel = ref<Record<'feishu' | 'dingtalk' | 'wecom', string>>({
  feishu: '',
  dingtalk: '',
  wecom: ''
})
const regStatusByChannel = ref<Record<'feishu' | 'dingtalk' | 'wecom', string>>({
  feishu: '',
  dingtalk: '',
  wecom: ''
})
const regBusy = ref<Record<'feishu' | 'dingtalk' | 'wecom', boolean>>({
  feishu: false,
  dingtalk: false,
  wecom: false
})

const config = ref<ChannelsConfig>({
  meta: {
    publicBaseUrl: '',
    sessionReset: { idleMinutes: DEFAULT_CHANNEL_IDLE_MINUTES },
    imOutbound: { sendIntermediateText: true, sendToolCalls: true }
  },
  feishu: { default: defaultFeishu() },
  dingtalk: { default: defaultDingtalk() },
  wecom: { default: defaultWecom() },
  weixin: { default: defaultWeixin() }
})

const webhookUrls = ref<Record<string, string>>({})
const connectionByTab = ref<Partial<Record<ChannelTab, boolean>>>({})
const tauriMode = isTauriRuntime()
let connectionPollId: ReturnType<typeof setInterval> | undefined

const COMMON_SETTINGS_HELP =
  '以下配置对所有 IM 通道（微信、飞书、企微、钉钉）生效。'

const SESSION_RESET_HELP =
  'IM 中发送 /new、/reset、新对话 或 重新开始 可手动开新会话。下方为空闲自动重置，默认 60 分钟，0 表示关闭。'

const IM_OUTBOUND_HELP =
  '控制 Agent 运行过程中推送到 IM 客户的消息。最终回复仍会发送；中间文字与工具进度默认均开启。'

const PUBLIC_BASE_URL_HELP =
  '各通道启用 Webhook 模式时需要填写，用于生成平台回调地址。'

const PAIRING_HELP =
  'DM 策略为配对模式时，陌生用户会收到配对码，在此输入并批准后可开始对话。'

const CONNECTION_MODE_LABELS: Record<ChannelTab, string> = {
  weixin: 'iLink 长轮询',
  feishu: 'WSS 长连接',
  wecom: 'WSS 长连接',
  dingtalk: 'Stream 长连接'
}

const tabHints: Record<ChannelTab, string> = {
  weixin: '使用微信 App 扫描二维码，在手机上确认登录',
  feishu: '使用飞书 App 扫描二维码，按提示完成应用授权',
  wecom: '使用企业微信 App 扫描二维码，点击「一键创建智能机器人」',
  dingtalk: '使用钉钉 App 扫描二维码，点击「一键创建新机器人」'
}

function normalizeAllConnectionModes() {
  for (const ch of ['feishu', 'dingtalk', 'wecom'] as const) {
    const acc = config.value[ch]?.default
    if (acc) {
      acc.connectionMode = normalizeConnectionMode(acc.connectionMode)
    }
  }
}

function sessionTextForTab(tab: ChannelTab): string {
  if (tab === 'weixin') return weixinLoginStatus.value
  if (tab === 'wecom') return regStatusByChannel.value.wecom
  return regStatusByChannel.value[tab] ?? ''
}

const channelStatusByTab = computed(() => {
  const out = {} as Record<ChannelTab, ReturnType<typeof resolveChannelStatus>>
  for (const { id } of CHANNEL_TABS) {
    out[id] = resolveChannelStatus(
      id,
      config.value,
      weixinLoggedIn.value,
      sessionTextForTab(id),
      connectionByTab.value[id]
    )
  }
  return out
})

const activeStatus = computed(() => channelStatusByTab.value[activeTab.value])

const statusToneClass = computed(() => {
  switch (activeStatus.value.tone) {
    case 'success':
      return 'text-green-600'
    case 'warn':
      return 'text-amber-600'
    case 'error':
      return 'text-red-500'
    default:
      return 'text-muted'
  }
})

const activeQrBase64 = computed(() => {
  if (activeTab.value === 'weixin') return weixinQr.value
  if (activeTab.value === 'feishu') return qrByChannel.value.feishu
  if (activeTab.value === 'dingtalk') return qrByChannel.value.dingtalk
  if (activeTab.value === 'wecom') return qrByChannel.value.wecom
  return ''
})

const activeScanBusy = computed(() => {
  if (activeTab.value === 'weixin') return weixinBusy.value
  return regBusy.value[activeTab.value as 'feishu' | 'dingtalk' | 'wecom'] ?? false
})

const activeHasCredentials = computed(
  () => activeStatus.value.setup !== 'no_credentials'
)

const activeConnectDisabled = computed(() => {
  if (connecting.value || saving.value || loading.value) return true
  if (activeTab.value === 'weixin' && !tauriMode) return true
  const setup = activeStatus.value.setup
  return setup === 'no_credentials' || setup === 'webhook_mode'
})

const activeConnectLabel = computed(() => {
  if (connecting.value) return '连接中…'
  if (connectionByTab.value[activeTab.value]) return '重新连接'
  return '连接'
})

const activeScanLabel = computed(() => {
  if (activeScanBusy.value) return '处理中…'
  if (activeQrBase64.value || activeHasCredentials.value) return '重新扫码'
  return '开始扫码'
})

const activeConnectionLabel = computed(() => {
  if (activeTab.value === 'weixin') return CONNECTION_MODE_LABELS.weixin
  const acc = config.value[activeTab.value]?.default
  if (acc?.connectionMode === 'webhook') return 'HTTP 回调'
  return CONNECTION_MODE_LABELS[activeTab.value]
})

const idleMinutes = computed(
  () => config.value.meta?.sessionReset?.idleMinutes ?? DEFAULT_CHANNEL_IDLE_MINUTES
)

function setIdleMinutes(raw: string) {
  const parsed = Number.parseInt(raw, 10)
  const minutes = Number.isFinite(parsed) && parsed >= 0 ? parsed : DEFAULT_CHANNEL_IDLE_MINUTES
  if (!config.value.meta) config.value.meta = { publicBaseUrl: '' }
  config.value.meta.sessionReset = { idleMinutes: minutes }
}

const sendIntermediateText = computed({
  get: () => config.value.meta?.imOutbound?.sendIntermediateText ?? true,
  set: (value: boolean) => {
    if (!config.value.meta) config.value.meta = { publicBaseUrl: '' }
    if (!config.value.meta.imOutbound) {
      config.value.meta.imOutbound = { sendIntermediateText: true, sendToolCalls: true }
    }
    config.value.meta.imOutbound.sendIntermediateText = value
  }
})

const sendToolCalls = computed({
  get: () => config.value.meta?.imOutbound?.sendToolCalls ?? true,
  set: (value: boolean) => {
    if (!config.value.meta) config.value.meta = { publicBaseUrl: '' }
    if (!config.value.meta.imOutbound) {
      config.value.meta.imOutbound = { sendIntermediateText: true, sendToolCalls: true }
    }
    config.value.meta.imOutbound.sendToolCalls = value
  }
})

function defaultFeishu(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'websocket',
    dmPolicy: 'pairing',
    groupPolicy: 'allowlist',
    requireMention: true
  }
}

function defaultDingtalk(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'websocket',
    dmPolicy: 'pairing',
    groupPolicy: 'allowlist',
    requireMention: true
  }
}

function defaultWecom(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'websocket',
    dmPolicy: 'pairing',
    groupPolicy: 'allowlist'
  }
}

function defaultWeixin(): ChannelAccountConfig {
  return { enabled: false }
}

function urlKey(channel: string, accountId = 'default') {
  return `${channel}:${accountId}`
}

function mergeConfig(loaded: ChannelsConfig) {
  const feishuAcc = { ...defaultFeishu(), ...loaded.feishu?.default }
  const dingtalkAcc = { ...defaultDingtalk(), ...loaded.dingtalk?.default }
  const wecomAcc = { ...defaultWecom(), ...loaded.wecom?.default }
  const feishuMode = preferConnectionMode('feishu', feishuAcc)
  const dingtalkMode = preferConnectionMode('dingtalk', dingtalkAcc)
  const wecomMode = preferConnectionMode('wecom', wecomAcc)

  config.value = {
    meta: {
      publicBaseUrl: loaded.meta?.publicBaseUrl ?? '',
      sessionReset: {
        idleMinutes: loaded.meta?.sessionReset?.idleMinutes ?? DEFAULT_CHANNEL_IDLE_MINUTES
      },
      imOutbound: {
        sendIntermediateText: loaded.meta?.imOutbound?.sendIntermediateText ?? true,
        sendToolCalls: loaded.meta?.imOutbound?.sendToolCalls ?? true
      }
    },
    feishu: {
      default: {
        ...feishuAcc,
        connectionMode: feishuMode
      }
    },
    dingtalk: {
      default: {
        ...dingtalkAcc,
        connectionMode: dingtalkMode
      }
    },
    wecom: {
      default: {
        ...wecomAcc,
        connectionMode: wecomMode
      }
    },
    weixin: { default: { ...defaultWeixin(), ...loaded.weixin?.default } }
  }
  config.value = sanitizeChannelsConfig(config.value)
}

async function refreshConnectionStatus() {
  try {
    const status = await listChannelStatus()
    const next: Partial<Record<ChannelTab, boolean>> = {}
    for (const item of status.channels) {
      if (item.accountId === 'default' && CHANNEL_TABS.some(t => t.id === item.channel)) {
        next[item.channel as ChannelTab] = item.connected
      }
    }
    connectionByTab.value = next
  } catch {
    // 连接态查询失败不阻断配置页
  }
}

async function refresh() {
  loading.value = true
  error.value = ''
  try {
    const [loaded, status] = await Promise.all([getChannelsConfig(), listChannelStatus()])
    mergeConfig(loaded)
    const nextConn: Partial<Record<ChannelTab, boolean>> = {}
    for (const item of status.channels) {
      webhookUrls.value[urlKey(item.channel, item.accountId)] = item.webhookUrl
      if (item.accountId === 'default' && CHANNEL_TABS.some(t => t.id === item.channel)) {
        nextConn[item.channel as ChannelTab] = item.connected
      }
    }
    connectionByTab.value = nextConn
    await refreshWeixinLoginState()
    // 扫码会话状态仅当次有效，刷新后清掉避免误显示
    regStatusByChannel.value = { feishu: '', dingtalk: '', wecom: '' }
    weixinLoginStatus.value = ''
    weixinQr.value = ''
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

async function persistConfig(restartMonitors = false) {
  normalizeAllConnectionModes()
  config.value = sanitizeChannelsConfig(config.value)
  await updateChannelsConfig(config.value, { restartMonitors })
}

/** 供设置页底部「保存」调用：仅持久化配置，不强制连接 */
async function save() {
  saving.value = true
  error.value = ''
  try {
    await persistConfig()
    await refresh()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    throw e
  } finally {
    saving.value = false
  }
}

function enableChannelLocally(channel: ChannelTab) {
  const acc = config.value[channel]?.default
  if (acc) {
    acc.enabled = true
  }
}

function setChannelSessionMessage(tab: ChannelTab, message: string) {
  if (tab === 'weixin') {
    weixinLoginStatus.value = message
    return
  }
  regStatusByChannel.value[tab] = message
}

/** 保存配置并重启 monitor 后，轮询直到长连接就绪或超时 */
async function waitForChannelConnection(
  tab: ChannelTab,
  opts: { attempts?: number; intervalMs?: number } = {}
): Promise<boolean> {
  const attempts = opts.attempts ?? 20
  const intervalMs = opts.intervalMs ?? 1500
  for (let i = 0; i < attempts; i++) {
    await refreshConnectionStatus()
    if (connectionByTab.value[tab] === true) return true
    if (i < attempts - 1) {
      await new Promise(r => setTimeout(r, intervalMs))
    }
  }
  await refreshConnectionStatus()
  return connectionByTab.value[tab] === true
}

/** 指定通道：启用 + 保存 + 启动 monitor + 等待连接结果 */
async function connectChannel(tab: ChannelTab) {
  connecting.value = true
  error.value = ''
  try {
    enableChannelLocally(tab)
    setChannelSessionMessage(tab, '正在连接…')
    await persistConfig(true)
    const connected = await waitForChannelConnection(tab)
    setChannelSessionMessage(
      tab,
      connected ? '连接成功，运行中可收发消息' : '连接未建立，请稍后点击「连接」重试'
    )
    if (!connected) {
      error.value = '通道在预期时间内未建立连接，请检查凭证或网络后重试'
    }
  } catch (e) {
    setChannelSessionMessage(tab, '连接失败')
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    connecting.value = false
  }
}

async function connectActiveChannel() {
  await connectChannel(activeTab.value)
}

async function copyWebhook(channel: string, accountId = 'default') {
  try {
    const url = await getChannelWebhookUrl(channel, accountId)
    webhookUrls.value[urlKey(channel, accountId)] = url
    await navigator.clipboard.writeText(url)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

function weixinStatusLabel(status: string): string {
  switch (status) {
    case 'pending':
      return '等待扫码'
    case 'scanned':
      return '已扫码，请在手机上确认'
    case 'confirmed':
      return '登录成功'
    case 'expired':
      return '二维码已过期'
    case 'failed':
      return '登录失败'
    default:
      return status
  }
}

function registrationStatusLabel(status: string): string {
  switch (status) {
    case 'pending':
      return '等待扫码'
    case 'success':
      return '授权成功，凭证已填入'
    case 'denied':
      return '用户拒绝授权'
    case 'expired':
      return '二维码已过期'
    case 'timeout':
      return '授权超时'
    case 'failed':
      return '授权失败'
    default:
      return status
  }
}

async function refreshWeixinLoginState() {
  if (!tauriMode) return
  try {
    weixinLoggedIn.value = await hasWeixinCredentials('default')
  } catch {
    weixinLoggedIn.value = false
  }
}

async function startWeixinQr() {
  error.value = ''
  weixinBusy.value = true
  weixinLoginStatus.value = '正在获取二维码…'
  try {
    const session = await startWeixinLogin('default')
    weixinQr.value = session.qrcodePngBase64
    weixinLoginStatus.value = weixinStatusLabel(session.status || 'pending')
    void pollWeixinStatus()
  } catch (e) {
    weixinLoginStatus.value = ''
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    weixinBusy.value = false
  }
}

function applyRegistrationCredentials(
  channel: 'feishu' | 'dingtalk' | 'wecom',
  session: {
    appId?: string
    appSecret?: string
    clientId?: string
    clientSecret?: string
    botId?: string
    secret?: string
  }
) {
  if (channel === 'feishu' && session.appId && session.appSecret) {
    config.value.feishu!.default.appId = session.appId
    config.value.feishu!.default.appSecret = session.appSecret
    config.value.feishu!.default.connectionMode = 'websocket'
    return
  }
  if (channel === 'dingtalk' && session.clientId && session.clientSecret) {
    config.value.dingtalk!.default.clientId = session.clientId
    config.value.dingtalk!.default.clientSecret = session.clientSecret
    config.value.dingtalk!.default.connectionMode = 'websocket'
    return
  }
  if (channel === 'wecom' && session.botId && session.secret) {
    config.value.wecom!.default.botId = session.botId
    config.value.wecom!.default.secret = session.secret
    config.value.wecom!.default.connectionMode = 'websocket'
  }
}

async function pollChannelRegistration(channel: 'feishu' | 'dingtalk' | 'wecom') {
  for (let i = 0; i < 180; i++) {
    await new Promise(r => setTimeout(r, 2000))
    const session = await getChannelRegistrationStatus(channel, 'default')
    if (!session) continue
    regStatusByChannel.value[channel] = registrationStatusLabel(session.status)
    if (session.status === 'success') {
      qrByChannel.value[channel] = ''
      applyRegistrationCredentials(channel, session)
      regStatusByChannel.value[channel] = '授权成功，正在连接…'
      await connectChannel(channel)
      return
    }
    if (['denied', 'expired', 'timeout', 'failed'].includes(session.status)) {
      qrByChannel.value[channel] = ''
      error.value = session.errorMessage || registrationStatusLabel(session.status)
      return
    }
  }
  regStatusByChannel.value[channel] = '授权超时，请重新扫码'
}

async function startQrRegistration(channel: 'feishu' | 'dingtalk' | 'wecom') {
  error.value = ''
  regBusy.value[channel] = true
  regStatusByChannel.value[channel] = '正在获取二维码…'
  try {
    const session = await startChannelRegistration(channel, 'default')
    qrByChannel.value[channel] = session.qrcodePngBase64
    regStatusByChannel.value[channel] = registrationStatusLabel(session.status || 'pending')
    void pollChannelRegistration(channel)
  } catch (e) {
    qrByChannel.value[channel] = ''
    regStatusByChannel.value[channel] = ''
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    regBusy.value[channel] = false
  }
}

async function startActiveScan() {
  if (activeTab.value === 'weixin') {
    await startWeixinQr()
    return
  }
  await startQrRegistration(activeTab.value)
}

async function pollWeixinStatus() {
  for (let i = 0; i < 60; i++) {
    await new Promise(r => setTimeout(r, 2000))
    const s = await getWeixinLoginStatus('default')
    if (!s) {
      await refreshWeixinLoginState()
      if (weixinLoggedIn.value) {
        weixinQr.value = ''
        weixinLoginStatus.value = '登录成功，正在连接…'
        await connectChannel('weixin')
      }
      return
    }
    weixinLoginStatus.value = weixinStatusLabel(s.status)
    if (s.status === 'confirmed') {
      weixinQr.value = ''
      weixinLoggedIn.value = true
      weixinLoginStatus.value = '登录成功，正在连接…'
      await connectChannel('weixin')
      return
    }
    if (s.status === 'expired' || s.status === 'failed') {
      weixinQr.value = ''
      error.value = weixinStatusLabel(s.status)
      return
    }
  }
  weixinLoginStatus.value = '登录超时，请重新扫码'
}

async function approvePairingAuto() {
  if (!pairingCode.value.trim()) return
  pairingSuccess.value = ''
  try {
    const channel = await approveChannelPairingAny('default', pairingCode.value.trim())
    pairingCode.value = ''
    error.value = ''
    const label = CHANNEL_TABS.find(t => t.id === channel)?.label ?? channel
    pairingSuccess.value = `配对成功（${label}），请重新发送消息。`
  } catch (e) {
    pairingSuccess.value = ''
    error.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(() => {
  void refresh()
  connectionPollId = window.setInterval(() => {
    void refreshConnectionStatus()
  }, 5000)
})

onUnmounted(() => {
  if (connectionPollId !== undefined) {
    window.clearInterval(connectionPollId)
  }
})

defineExpose({ save })
</script>

<template>
  <div class="space-y-5">
    <p class="text-sm text-muted">
      扫码授权成功后会自动连接；手填凭证需点「连接」。底部「保存」写入全部通道配置。飞书 / 企微为 WSS 长连接，钉钉为 Stream 长连接。
    </p>

    <div v-if="error" class="rounded-lg border border-red-500/30 bg-red-500/5 px-3 py-2 text-sm text-red-500">
      {{ error }}
    </div>
    <div v-if="pairingSuccess" class="rounded-lg border border-green-500/30 bg-green-500/5 px-3 py-2 text-sm text-green-600">
      {{ pairingSuccess }}
    </div>

    <div class="channel-tabs" role="tablist">
      <button
        v-for="tab in CHANNEL_TABS"
        :key="tab.id"
        type="button"
        role="tab"
        class="channel-tab"
        :class="{ active: activeTab === tab.id }"
        :aria-selected="activeTab === tab.id"
        @click="activeTab = tab.id"
      >
        <span>{{ tab.label }}</span>
        <span
          v-if="channelStatusByTab[tab.id].tabDot !== 'none'"
          :class="channelStatusByTab[tab.id].tabDot === 'ok' ? 'tab-dot' : 'tab-dot-warn'"
          :title="channelStatusByTab[tab.id].setupLabel"
        />
      </button>
    </div>

    <div class="rounded-xl border border-border overflow-hidden">
      <div class="flex items-center justify-between gap-3 border-b border-border px-4 py-3 bg-[hsl(var(--card-elevated))]">
        <div>
          <h3 class="text-sm font-medium">
            {{ CHANNEL_TABS.find(t => t.id === activeTab)?.label }}
          </h3>
          <div class="flex items-center gap-2 mt-0.5">
            <span class="mode-badge">{{ activeConnectionLabel }}</span>
            <span class="text-xs text-muted">{{ tabHints[activeTab] }}</span>
          </div>
        </div>
        <label class="flex items-center gap-2 text-xs shrink-0">
          <span class="text-muted">启用</span>
          <input
            v-if="activeTab === 'weixin'"
            v-model="config.weixin!.default.enabled"
            type="checkbox"
          />
          <input
            v-else-if="activeTab === 'feishu'"
            v-model="config.feishu!.default.enabled"
            type="checkbox"
          />
          <input
            v-else-if="activeTab === 'wecom'"
            v-model="config.wecom!.default.enabled"
            type="checkbox"
          />
          <input
            v-else
            v-model="config.dingtalk!.default.enabled"
            type="checkbox"
          />
        </label>
      </div>

      <div class="p-5">
        <div class="qr-hero">
          <div v-if="activeQrBase64" class="qr-frame">
            <img
              :src="`data:image/png;base64,${activeQrBase64}`"
              :alt="`${activeTab} QR`"
              class="qr-image"
            />
          </div>
          <div v-else class="qr-placeholder">
            <QrCode class="w-10 h-10 text-muted/50" />
            <p class="text-sm text-muted mt-3">扫码连接</p>
          </div>

          <p
            v-if="activeStatus.displayText"
            class="text-sm mt-4"
            :class="statusToneClass"
          >
            {{ activeStatus.displayText }}
          </p>

          <div class="btn-row">
            <button
              type="button"
              class="btn-scan"
              :disabled="activeScanBusy || (activeTab === 'weixin' && !tauriMode)"
              @click="startActiveScan"
            >
              <QrCode class="w-4 h-4" />
              {{ activeScanLabel }}
            </button>
            <button
              type="button"
              class="btn-connect"
              :class="{ 'btn-connect-live': connectionByTab[activeTab] }"
              :disabled="activeConnectDisabled"
              @click="connectActiveChannel"
            >
              <Plug class="w-4 h-4" />
              {{ activeConnectLabel }}
            </button>
          </div>

          <p v-if="activeTab === 'weixin' && !tauriMode" class="text-xs text-muted mt-2">
            微信扫码登录仅支持桌面客户端
          </p>
        </div>

        <details class="manual-section mt-5">
          <summary class="manual-summary">
            <span>手动填写凭证</span>
            <ChevronDown class="w-4 h-4 summary-chevron" />
          </summary>

          <div class="manual-body space-y-3">
            <template v-if="activeTab === 'weixin'">
              <p class="text-xs text-muted">微信通过扫码登录获取凭证，无需手填。</p>
            </template>

            <template v-else-if="activeTab === 'feishu'">
              <input
                v-model="config.feishu!.default.appId"
                placeholder="App ID（如 cli_xxxxxxxx）"
                class="field placeholder:text-muted"
              />
              <input
                v-model="config.feishu!.default.appSecret"
                placeholder="App Secret"
                class="field placeholder:text-muted"
              />
            </template>

            <template v-else-if="activeTab === 'wecom'">
              <input
                v-model="config.wecom!.default.botId"
                placeholder="智能机器人 Bot ID"
                class="field placeholder:text-muted"
              />
              <input
                v-model="config.wecom!.default.secret"
                placeholder="Bot Secret"
                class="field placeholder:text-muted"
              />
              <input
                v-model="config.wecom!.default.websocketUrl"
                placeholder="WSS 地址（留空则用 wss://openws.work.weixin.qq.com）"
                class="field placeholder:text-muted"
              />
            </template>

            <template v-else>
              <input
                v-model="config.dingtalk!.default.clientId"
                placeholder="Client ID / AppKey"
                class="field placeholder:text-muted"
              />
              <input
                v-model="config.dingtalk!.default.clientSecret"
                placeholder="Client Secret / AppSecret"
                class="field placeholder:text-muted"
              />
            </template>
          </div>
        </details>

        <details class="manual-section mt-3">
          <summary class="manual-summary">
            <span>高级设置（Webhook 备选）</span>
            <ChevronDown class="w-4 h-4 summary-chevron" />
          </summary>
          <div class="manual-body space-y-4">
            <p class="text-xs text-muted">
              默认使用 WSS / Stream 长连接。仅在需要 HTTP 回调时才启用 Webhook 模式。
            </p>

            <div v-if="activeTab === 'feishu'" class="space-y-2">
              <label class="flex items-center gap-2 text-xs cursor-pointer">
                <input
                  type="checkbox"
                  :checked="config.feishu!.default.connectionMode === 'webhook'"
                  @change="config.feishu!.default.connectionMode = ($event.target as HTMLInputElement).checked ? 'webhook' : 'websocket'"
                />
                启用 HTTP 回调（飞书）
              </label>
              <template v-if="config.feishu!.default.connectionMode === 'webhook'">
                <input v-model="config.feishu!.default.encryptKey" placeholder="Encrypt Key" class="field" />
                <button type="button" class="btn-ghost" @click="copyWebhook('feishu')">
                  <Copy class="w-3.5 h-3.5" />
                  复制 Webhook URL
                </button>
                <p v-if="webhookUrls[urlKey('feishu')]" class="text-xs text-muted break-all">
                  {{ webhookUrls[urlKey('feishu')] }}
                </p>
              </template>
            </div>

            <div v-else-if="activeTab === 'wecom'" class="space-y-2">
              <label class="flex items-center gap-2 text-xs cursor-pointer">
                <input
                  type="checkbox"
                  :checked="config.wecom!.default.connectionMode === 'webhook'"
                  @change="config.wecom!.default.connectionMode = ($event.target as HTMLInputElement).checked ? 'webhook' : 'websocket'"
                />
                启用 HTTP 回调（企微 Agent 模式）
              </label>
              <template v-if="config.wecom!.default.connectionMode === 'webhook'">
                <input v-model="config.wecom!.default.corpId" placeholder="Corp ID" class="field" />
                <input v-model="config.wecom!.default.agentId" placeholder="Agent ID" class="field" />
                <input v-model="config.wecom!.default.secret" placeholder="应用 Secret" class="field" />
                <input v-model="config.wecom!.default.token" placeholder="回调 Token" class="field" />
                <input v-model="config.wecom!.default.encodingAesKey" placeholder="Encoding AES Key" class="field" />
                <button type="button" class="btn-ghost" @click="copyWebhook('wecom')">
                  <Copy class="w-3.5 h-3.5" />
                  复制 Webhook URL
                </button>
              </template>
            </div>

            <div v-else-if="activeTab === 'dingtalk'" class="space-y-2">
              <label class="flex items-center gap-2 text-xs cursor-pointer">
                <input
                  type="checkbox"
                  :checked="config.dingtalk!.default.connectionMode === 'webhook'"
                  @change="config.dingtalk!.default.connectionMode = ($event.target as HTMLInputElement).checked ? 'webhook' : 'websocket'"
                />
                启用 HTTP 回调（钉钉）
              </label>
              <template v-if="config.dingtalk!.default.connectionMode === 'webhook'">
                <button type="button" class="btn-ghost" @click="copyWebhook('dingtalk')">
                  <Copy class="w-3.5 h-3.5" />
                  复制 Webhook URL
                </button>
                <p v-if="webhookUrls[urlKey('dingtalk')]" class="text-xs text-muted break-all">
                  {{ webhookUrls[urlKey('dingtalk')] }}
                </p>
              </template>
            </div>

            <div v-else class="text-xs text-muted">微信仅支持 iLink 长轮询，无 Webhook 模式。</div>
          </div>
        </details>
      </div>
    </div>

    <div class="common-settings-card">
      <div class="common-settings-title flex items-center gap-1.5">
        <h3>通用设置</h3>
        <button
          type="button"
          class="field-help"
          :title="COMMON_SETTINGS_HELP"
          aria-label="通用设置说明"
          @click.stop
        >
          <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
        </button>
      </div>

      <section class="common-block">
        <div class="common-settings-grid">
          <div class="common-settings-cell">
            <div class="common-block-head">
              <h4 class="common-block-title">IM 出站推送</h4>
              <button
                type="button"
                class="field-help"
                :title="IM_OUTBOUND_HELP"
                aria-label="IM 出站推送说明"
                @click.stop
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <div class="common-pair-controls common-pair-controls-loose">
              <label class="common-check">
                <input v-model="sendIntermediateText" type="checkbox" class="rounded" />
                <span>中间文字</span>
              </label>
              <label class="common-check">
                <input v-model="sendToolCalls" type="checkbox" class="rounded" />
                <span>工具进度</span>
              </label>
            </div>
          </div>
          <div class="common-settings-cell common-settings-cell-aside">
            <div class="common-block-head">
              <h4 class="common-block-title">会话重置</h4>
              <button
                type="button"
                class="field-help"
                :title="SESSION_RESET_HELP"
                aria-label="会话重置说明"
                @click.stop
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <div class="common-aside-controls common-pair-controls common-session-input">
              <input
                type="number"
                min="0"
                class="field min-w-0 flex-1"
                placeholder="60"
                :value="idleMinutes"
                @input="setIdleMinutes(($event.target as HTMLInputElement).value)"
              />
              <span class="common-unit">分钟</span>
            </div>
          </div>
          <div class="common-settings-cell common-settings-cell-split">
            <div class="common-block-head">
              <h4 class="common-block-title">公网 Base URL</h4>
              <button
                type="button"
                class="field-help"
                :title="PUBLIC_BASE_URL_HELP"
                aria-label="公网 Base URL 说明"
                @click.stop
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <input
              v-model="config.meta!.publicBaseUrl"
              class="field"
              placeholder="https://pointer.example.com"
            />
          </div>
          <div class="common-settings-cell common-settings-cell-aside common-settings-cell-split">
            <div class="common-block-head">
              <h4 class="common-block-title">配对审批</h4>
              <button
                type="button"
                class="field-help"
                :title="PAIRING_HELP"
                aria-label="配对审批说明"
                @click.stop
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <div class="common-aside-controls common-pair-controls">
              <input v-model="pairingCode" placeholder="配对码" class="field min-w-0 flex-1" />
              <button type="button" class="btn-primary btn-compact shrink-0" @click="approvePairingAuto">
                批准
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>

    <div class="flex gap-2 pt-1">
      <button type="button" class="btn-ghost" :disabled="loading" @click="refresh">
        <RefreshCw class="w-3.5 h-3.5" />
        刷新状态
      </button>
    </div>
  </div>
</template>

<style scoped>
.channel-tabs {
  @apply flex gap-1 p-1 rounded-xl border border-border bg-[hsl(var(--card-elevated))];
}

.channel-tab {
  @apply relative flex-1 flex items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-sm text-muted transition-colors;
}

.channel-tab:hover {
  @apply text-foreground;
}

.channel-tab.active {
  @apply bg-background text-foreground font-medium shadow-sm;
}

.mode-badge {
  @apply inline-flex shrink-0 items-center rounded-md border border-border bg-background px-1.5 py-0.5 text-[10px] font-medium text-muted;
}

.tab-dot {
  @apply w-1.5 h-1.5 rounded-full bg-green-500;
}

.tab-dot-warn {
  @apply w-1.5 h-1.5 rounded-full bg-amber-500;
}

.qr-hero {
  @apply flex flex-col items-center text-center;
}

.qr-frame {
  @apply rounded-2xl border-2 border-border bg-white p-4 shadow-sm;
}

.qr-image {
  @apply w-52 h-52 object-contain;
}

.qr-placeholder {
  @apply flex flex-col items-center justify-center w-52 h-52 rounded-2xl border-2 border-dashed border-border bg-[hsl(var(--card-elevated))];
}

.btn-row {
  @apply mt-4 flex flex-wrap items-center justify-center gap-2;
}

.btn-scan {
  @apply inline-flex items-center gap-2 rounded-xl bg-accent px-5 py-2.5 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50;
}

.btn-connect {
  @apply inline-flex items-center gap-2 rounded-xl border border-border bg-background px-5 py-2.5 text-sm font-medium hover:bg-hover disabled:opacity-50;
}

.btn-connect-live {
  @apply border-green-500/40 text-green-700;
}

.common-settings-card {
  @apply rounded-xl border border-border;
}

.common-settings-title {
  @apply px-4 py-3 text-sm font-medium bg-[hsl(var(--card-elevated))] border-b border-border rounded-t-xl;
}

.common-block {
  @apply px-4 py-3 border-b border-border last:border-b-0;
}

.common-settings-grid {
  @apply grid grid-cols-1 gap-4 sm:grid-cols-2 sm:gap-x-0 sm:gap-y-0;
}

.common-settings-cell {
  @apply flex flex-col gap-1.5 min-w-0 sm:pr-4;
}

.common-settings-cell-aside {
  @apply sm:border-l sm:border-border sm:pl-4 sm:pr-3;
}

.common-settings-cell-split {
  @apply sm:mt-4 sm:pt-4 sm:border-t sm:border-border;
}

.common-aside-controls {
  @apply w-1/2 min-w-0 max-w-[11rem];
}

.common-pair-controls {
  @apply flex items-center min-h-[2.25rem] flex-nowrap gap-2;
}

.common-pair-controls-loose {
  @apply gap-x-4;
}

.common-session-input {
  @apply gap-1.5;
}

.common-unit {
  @apply text-xs text-muted shrink-0 leading-none;
}

.common-check {
  @apply flex items-center gap-1.5 text-sm cursor-pointer whitespace-nowrap;
}

.common-block-head {
  @apply flex items-center gap-1.5;
}

.common-block-title {
  @apply text-sm font-medium;
}

.field-help {
  @apply inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0;
}

.manual-section {
  @apply rounded-xl border border-border;
}

.manual-summary {
  @apply flex items-center justify-between gap-2 cursor-pointer select-none px-4 py-3 text-sm font-medium list-none;
}

.manual-summary::-webkit-details-marker {
  display: none;
}

.manual-body {
  @apply px-4 pb-4 border-t border-border pt-3;
}

.manual-section[open] .summary-chevron {
  transform: rotate(180deg);
}

.summary-chevron {
  @apply text-muted transition-transform;
}

.field {
  @apply w-full rounded-lg border border-border bg-background px-3 py-2 text-sm placeholder:text-muted;
}

.btn-ghost {
  @apply inline-flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-hover;
}

.btn-primary {
  @apply rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50;
}

.btn-compact {
  @apply px-3 py-1.5 text-xs;
}
</style>
