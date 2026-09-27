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
import { t } from '../i18n'
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

  const CHANNEL_TABS = computed(() => [
    { id: 'weixin' as ChannelTab, label: t('settings.channels.brands.weixin') },
    { id: 'feishu' as ChannelTab, label: t('settings.channels.brands.feishu') },
    { id: 'wecom' as ChannelTab, label: t('settings.channels.brands.wecom') },
    { id: 'dingtalk' as ChannelTab, label: t('settings.channels.brands.dingtalk') }
  ])

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

  const COMMON_SETTINGS_HELP = computed(() => t('settings.channels.commonHelp'))

  const SESSION_RESET_HELP = computed(() => t('settings.channels.sessionResetHelp'))

  const IM_OUTBOUND_HELP = computed(() => t('settings.channels.imOutboundHelp'))

  const PUBLIC_BASE_URL_HELP = computed(() => t('settings.channels.publicBaseUrlHelp'))

  const PAIRING_HELP = computed(() => t('settings.channels.pairingHelp'))

  const CONNECTION_MODE_LABELS = computed(() => ({
    weixin: t('settings.channels.modeIlink'),
    feishu: t('settings.channels.modeWss'),
    wecom: t('settings.channels.modeWss'),
    dingtalk: t('settings.channels.modeStream')
  } as Record<ChannelTab, string>))

  const tabHints = computed(() => ({
    weixin: t('settings.channels.hintWeixin'),
    feishu: t('settings.channels.hintFeishu'),
    wecom: t('settings.channels.hintWecom'),
    dingtalk: t('settings.channels.hintDingtalk')
  } as Record<ChannelTab, string>))

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
    for (const { id } of CHANNEL_TABS.value) {
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
    if (connecting.value) return t('settings.channels.connecting')
    if (connectionByTab.value[activeTab.value]) return t('settings.channels.reconnect')
    return t('settings.channels.connect')
  })

  const activeScanLabel = computed(() => {
    if (activeScanBusy.value) return t('settings.channels.processing')
    if (activeQrBase64.value || activeHasCredentials.value) return t('settings.channels.rescan')
    return t('settings.channels.startScan')
  })

  const activeConnectionLabel = computed(() => {
    if (activeTab.value === 'weixin') return CONNECTION_MODE_LABELS.value.weixin
    const acc = config.value[activeTab.value]?.default
    if (acc?.connectionMode === 'webhook') return t('settings.channels.modeHttp')
    return CONNECTION_MODE_LABELS.value[activeTab.value]
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
        if (item.accountId === 'default' && CHANNEL_TABS.value.some(tab => tab.id === item.channel)) {
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
        if (item.accountId === 'default' && CHANNEL_TABS.value.some(tab => tab.id === item.channel)) {
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
      setChannelSessionMessage(tab, t('settings.channels.connectingMsg'))
      await persistConfig(true)
      const connected = await waitForChannelConnection(tab)
      setChannelSessionMessage(
        tab,
        connected ? t('settings.channels.connectOk') : t('settings.channels.connectRetry')
      )
      if (!connected) {
        error.value = t('settings.channels.connectTimeout')
      }
    } catch (e) {
      setChannelSessionMessage(tab, t('settings.channels.connectFail'))
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
        return t('settings.channels.waitScan')
      case 'scanned':
        return t('settings.channels.scannedConfirm')
      case 'confirmed':
        return t('settings.channels.loginOk')
      case 'expired':
        return t('settings.channels.qrExpired')
      case 'failed':
        return t('settings.channels.loginFail')
      default:
        return status
    }
  }

  function registrationStatusLabel(status: string): string {
    switch (status) {
      case 'pending':
        return t('settings.channels.waitScan')
      case 'success':
        return t('settings.channels.authOkFilled')
      case 'denied':
        return t('settings.channels.authDenied')
      case 'expired':
        return t('settings.channels.qrExpired')
      case 'timeout':
        return t('settings.channels.authTimeout')
      case 'failed':
        return t('settings.channels.authFail')
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
    weixinLoginStatus.value = t('settings.channels.fetchingQr')
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
        regStatusByChannel.value[channel] = t('settings.channels.authOkConnecting')
        await connectChannel(channel)
        return
      }
      if (['denied', 'expired', 'timeout', 'failed'].includes(session.status)) {
        qrByChannel.value[channel] = ''
        error.value = session.errorMessage || registrationStatusLabel(session.status)
        return
      }
    }
    regStatusByChannel.value[channel] = t('settings.channels.authTimeoutRescan')
  }

  async function startQrRegistration(channel: 'feishu' | 'dingtalk' | 'wecom') {
    error.value = ''
    regBusy.value[channel] = true
    regStatusByChannel.value[channel] = t('settings.channels.fetchingQr')
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
          weixinLoginStatus.value = t('settings.channels.loginOkConnecting')
          await connectChannel('weixin')
        }
        return
      }
      weixinLoginStatus.value = weixinStatusLabel(s.status)
      if (s.status === 'confirmed') {
        weixinQr.value = ''
        weixinLoggedIn.value = true
        weixinLoginStatus.value = t('settings.channels.loginOkConnecting')
        await connectChannel('weixin')
        return
      }
      if (s.status === 'expired' || s.status === 'failed') {
        weixinQr.value = ''
        error.value = weixinStatusLabel(s.status)
        return
      }
    }
    weixinLoginStatus.value = t('settings.channels.loginTimeoutRescan')
  }

  async function approvePairingAuto() {
    if (!pairingCode.value.trim()) return
    pairingSuccess.value = ''
    try {
      const channel = await approveChannelPairingAny('default', pairingCode.value.trim())
      pairingCode.value = ''
      error.value = ''
      const label = CHANNEL_TABS.value.find(tab => tab.id === channel)?.label ?? channel
      pairingSuccess.value = t('settings.channels.pairingOk', { label })
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
