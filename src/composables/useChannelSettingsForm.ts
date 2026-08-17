import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  DEFAULT_CHANNEL_IDLE_MINUTES,
  type ChannelAccountConfig,
  type ChannelsConfig
} from '../types/channels'
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
} from '../lib/channels'
import { isTauriRuntime } from '../lib/runtime'
import {
  normalizeConnectionMode,
  preferConnectionMode,
  resolveChannelStatus,
  sanitizeChannelsConfig,
  type ChannelTab
} from '../lib/channel-setup-status'

export function useChannelSettingsForm() {
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
  let connectionPollId: number | undefined
  let channelAutosaveTimer: number | undefined
  let channelAutosaveReady = false
  let refreshingConfig = false
  let persistingConfig = false

  const COMMON_SETTINGS_HELP =
    '以下配置对所有 IM 通道（微信、飞书、企微、钉钉）生效。'

  const SESSION_RESET_HELP =
    'IM 中发送 /new、/reset、新对话 或 重新开始 可手动开新会话。下方为空闲自动重置，默认 60 分钟，0 表示关闭。'

  const IM_OUTBOUND_HELP =
    '控制 Agent 运行过程中推送到 IM 的消息。最终回复仍会发送；工具进度只推送「开始调用」，不推送完成/失败状态。'

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
        return 'text-success'
      case 'warn':
        return 'text-warning'
      case 'error':
        return 'text-danger'
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
      groupPolicy: 'allowlist',
      requireMention: true,
      dynamicAgents: {
        enabled: true,
        dmCreateAgent: true,
        groupEnabled: true,
        adminUsers: []
      }
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
    refreshingConfig = true
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
      refreshingConfig = false
      void nextTick(() => {
        channelAutosaveReady = true
      })
    }
  }

  async function persistConfig(restartMonitors = false) {
    normalizeAllConnectionModes()
    config.value = sanitizeChannelsConfig(config.value)
    persistingConfig = true
    try {
      await updateChannelsConfig(config.value, { restartMonitors })
    } finally {
      persistingConfig = false
    }
  }

  /** 配置变更即时持久化（防抖，避免输入框每键都写入配置）。 */
  function scheduleChannelAutosave() {
    if (!channelAutosaveReady || refreshingConfig || persistingConfig) return
    if (channelAutosaveTimer) window.clearTimeout(channelAutosaveTimer)
    channelAutosaveTimer = window.setTimeout(() => {
      channelAutosaveTimer = undefined
      void persistConfig().catch(e => {
        error.value = e instanceof Error ? e.message : String(e)
      })
    }, 400)
  }

  watch(config, scheduleChannelAutosave, { deep: true })

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


  return {
    loading,
    saving,
    connecting,
    error,
    pairingCode,
    pairingSuccess,
    CHANNEL_TABS,
    activeTab,
    weixinQr,
    weixinLoginStatus,
    weixinLoggedIn,
    weixinBusy,
    qrByChannel,
    regStatusByChannel,
    regBusy,
    config,
    webhookUrls,
    connectionByTab,
    tauriMode,
    COMMON_SETTINGS_HELP,
    SESSION_RESET_HELP,
    IM_OUTBOUND_HELP,
    PUBLIC_BASE_URL_HELP,
    PAIRING_HELP,
    CONNECTION_MODE_LABELS,
    tabHints,
    channelStatusByTab,
    activeStatus,
    statusToneClass,
    activeQrBase64,
    activeScanBusy,
    activeHasCredentials,
    activeConnectDisabled,
    activeConnectLabel,
    activeScanLabel,
    activeConnectionLabel,
    idleMinutes,
    setIdleMinutes,
    sendIntermediateText,
    sendToolCalls,
    urlKey,
    refresh,
    save,
    connectActiveChannel,
    copyWebhook,
    startActiveScan,
    approvePairingAuto,
  }
}
