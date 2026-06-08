<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Copy, RefreshCw } from 'lucide-vue-next'
import type { ChannelAccountConfig, ChannelsConfig } from '../../types/channels'
import {
  approveChannelPairing,
  approveChannelPairingAny,
  getChannelsConfig,
  getChannelWebhookUrl,
  getWeixinLoginStatus,
  hasWeixinCredentials,
  listChannelStatus,
  startWeixinLogin,
  updateChannelsConfig
} from '../../lib/channels'
import { isTauriRuntime } from '../../lib/runtime'

const loading = ref(false)
const saving = ref(false)
const error = ref('')
const pairingCode = ref('')
const pairingSuccess = ref('')

const CHANNEL_LABELS: Record<string, string> = {
  weixin: '微信',
  wecom: '企微',
  feishu: '飞书',
  dingtalk: '钉钉'
}
const weixinQr = ref('')
const weixinLoginStatus = ref('')
const weixinLoggedIn = ref(false)

const config = ref<ChannelsConfig>({
  meta: { publicBaseUrl: '' },
  feishu: { default: defaultFeishu() },
  dingtalk: { default: defaultDingtalk() },
  wecom: { default: defaultWecom() },
  weixin: { default: defaultWeixin() }
})

const webhookUrls = ref<Record<string, string>>({})
const tauriMode = isTauriRuntime()

function defaultFeishu(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'webhook',
    dmPolicy: 'pairing',
    groupPolicy: 'allowlist',
    requireMention: true
  }
}

function defaultDingtalk(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'webhook',
    dmPolicy: 'pairing',
    groupPolicy: 'allowlist',
    requireMention: true
  }
}

function defaultWecom(): ChannelAccountConfig {
  return {
    enabled: false,
    connectionMode: 'websocket',
    websocketUrl: 'wss://openws.work.weixin.qq.com',
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
  config.value = {
    meta: { publicBaseUrl: loaded.meta?.publicBaseUrl ?? '' },
    feishu: { default: { ...defaultFeishu(), ...loaded.feishu?.default } },
    dingtalk: { default: { ...defaultDingtalk(), ...loaded.dingtalk?.default } },
    wecom: { default: { ...defaultWecom(), ...loaded.wecom?.default } },
    weixin: { default: { ...defaultWeixin(), ...loaded.weixin?.default } }
  }
}

async function refresh() {
  loading.value = true
  error.value = ''
  try {
    const [loaded, status] = await Promise.all([getChannelsConfig(), listChannelStatus()])
    mergeConfig(loaded)
    for (const item of status.channels) {
      webhookUrls.value[urlKey(item.channel, item.accountId)] = item.webhookUrl
    }
    await refreshWeixinLoginState()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

async function save() {
  saving.value = true
  error.value = ''
  try {
    await updateChannelsConfig(config.value)
    await refresh()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
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
      return '等待扫码（请用微信 App 扫描）'
    case 'scanned':
      return '已扫码，请在手机上确认登录'
    case 'confirmed':
      return '登录成功，微信通道已就绪'
    case 'expired':
      return '二维码已过期，请重新获取'
    case 'failed':
      return '登录失败，请重试'
    default:
      return status
  }
}

async function refreshWeixinLoginState() {
  if (!tauriMode) return
  try {
    weixinLoggedIn.value = await hasWeixinCredentials('default')
    if (weixinLoggedIn.value && !weixinQr.value) {
      weixinLoginStatus.value = '已登录（本地凭证有效）'
    }
  } catch {
    weixinLoggedIn.value = false
  }
}

async function startWeixinQr() {
  error.value = ''
  weixinLoginStatus.value = '正在获取二维码…'
  try {
    const session = await startWeixinLogin('default')
    weixinQr.value = session.qrcodePngBase64
    weixinLoginStatus.value = weixinStatusLabel(session.status || 'pending')
    void pollWeixinStatus()
  } catch (e) {
    weixinLoginStatus.value = ''
    error.value = e instanceof Error ? e.message : String(e)
  }
}

async function pollWeixinStatus() {
  for (let i = 0; i < 60; i++) {
    await new Promise(r => setTimeout(r, 2000))
    const s = await getWeixinLoginStatus('default')
    if (!s) {
      await refreshWeixinLoginState()
      if (weixinLoggedIn.value) {
        weixinQr.value = ''
        weixinLoginStatus.value = '登录成功，微信通道已就绪'
      }
      return
    }
    weixinLoginStatus.value = weixinStatusLabel(s.status)
    if (s.status === 'confirmed') {
      weixinQr.value = ''
      weixinLoggedIn.value = true
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

async function approvePairing(channel: string) {
  if (!pairingCode.value.trim()) return
  pairingSuccess.value = ''
  try {
    await approveChannelPairing(channel, 'default', pairingCode.value.trim())
    pairingCode.value = ''
    error.value = ''
    const label = CHANNEL_LABELS[channel] ?? channel
    pairingSuccess.value = `配对成功（${label}），请重新发送消息。`
  } catch (e) {
    pairingSuccess.value = ''
    error.value = e instanceof Error ? e.message : String(e)
  }
}

async function approvePairingAuto() {
  if (!pairingCode.value.trim()) return
  pairingSuccess.value = ''
  try {
    const channel = await approveChannelPairingAny('default', pairingCode.value.trim())
    pairingCode.value = ''
    error.value = ''
    const label = CHANNEL_LABELS[channel] ?? channel
    pairingSuccess.value = `配对成功（${label}），请重新发送消息。`
  } catch (e) {
    pairingSuccess.value = ''
    error.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(() => {
  void refresh()
  void refreshWeixinLoginState()
})

defineExpose({ save })
</script>

<template>
  <div class="space-y-6">
    <p class="text-sm text-muted">
      IM 通道通过 pointer-server 的 Webhook 接收消息。请配置公网回调地址并启动 Web 服务端。
    </p>
    <p v-if="tauriMode" class="text-xs text-muted">
      桌面端可本地保存配置；Webhook 入站与 Agent 回复需公网部署 pointer-server 并填写 publicBaseUrl。
    </p>

    <div v-if="error" class="text-sm text-red-500">{{ error }}</div>
    <div v-if="pairingSuccess" class="text-sm text-green-600">{{ pairingSuccess }}</div>

    <div class="space-y-2">
      <label class="text-xs text-muted">公网 Base URL</label>
      <input
        v-model="config.meta!.publicBaseUrl"
        class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm"
        placeholder="https://pointer.example.com"
      />
    </div>

    <div class="rounded-xl border border-border p-4 space-y-3">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium">飞书</h3>
        <label class="flex items-center gap-2 text-xs">
          <input v-model="config.feishu!.default.enabled" type="checkbox" />
          启用
        </label>
      </div>
      <input v-model="config.feishu!.default.appId" placeholder="App ID" class="field" />
      <input v-model="config.feishu!.default.appSecret" placeholder="App Secret" class="field" />
      <input v-model="config.feishu!.default.encryptKey" placeholder="Encrypt Key" class="field" />
      <button type="button" class="btn-ghost" @click="copyWebhook('feishu')">
        <Copy class="w-3.5 h-3.5" />
        复制 Webhook URL
      </button>
      <p v-if="webhookUrls[urlKey('feishu')]" class="text-xs text-muted break-all">
        {{ webhookUrls[urlKey('feishu')] }}
      </p>
    </div>

    <div class="rounded-xl border border-border p-4 space-y-3">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium">钉钉</h3>
        <label class="flex items-center gap-2 text-xs">
          <input v-model="config.dingtalk!.default.enabled" type="checkbox" />
          启用
        </label>
      </div>
      <input v-model="config.dingtalk!.default.clientId" placeholder="Client ID / AppKey" class="field" />
      <input v-model="config.dingtalk!.default.clientSecret" placeholder="Client Secret" class="field" />
      <button type="button" class="btn-ghost" @click="copyWebhook('dingtalk')">
        <Copy class="w-3.5 h-3.5" />
        复制 Webhook URL
      </button>
    </div>

    <div class="rounded-xl border border-border p-4 space-y-3">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium">企微</h3>
        <label class="flex items-center gap-2 text-xs">
          <input v-model="config.wecom!.default.enabled" type="checkbox" />
          启用
        </label>
      </div>
      <div class="flex gap-2">
        <label class="flex items-center gap-1.5 text-xs cursor-pointer">
          <input v-model="config.wecom!.default.connectionMode" type="radio" value="websocket" />
          WSS 长连接（推荐）
        </label>
        <label class="flex items-center gap-1.5 text-xs cursor-pointer">
          <input v-model="config.wecom!.default.connectionMode" type="radio" value="webhook" />
          HTTP 回调
        </label>
      </div>
      <template v-if="config.wecom!.default.connectionMode === 'websocket'">
        <p class="text-xs text-muted">智能机器人 Bot 模式：无需公网 Webhook，客户端主动连企微 WSS。</p>
        <input v-model="config.wecom!.default.botId" placeholder="Bot ID" class="field" />
        <input v-model="config.wecom!.default.secret" placeholder="Bot Secret" class="field" />
        <input
          v-model="config.wecom!.default.websocketUrl"
          placeholder="WSS 地址（默认 wss://openws.work.weixin.qq.com）"
          class="field"
        />
      </template>
      <template v-else>
        <p class="text-xs text-muted">自建应用 Agent 模式：需公网 HTTPS 回调。</p>
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

    <div class="rounded-xl border border-border p-4 space-y-3">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium">微信（iLink 长轮询）</h3>
        <label class="flex items-center gap-2 text-xs">
          <input v-model="config.weixin!.default.enabled" type="checkbox" />
          启用
        </label>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <button type="button" class="btn-ghost" @click="startWeixinQr">
          {{ weixinLoggedIn ? '重新扫码登录' : '扫码登录' }}
        </button>
        <span
          v-if="weixinLoginStatus"
          class="text-xs"
          :class="weixinLoggedIn ? 'text-green-600' : 'text-muted'"
        >
          {{ weixinLoginStatus }}
        </span>
      </div>
      <img v-if="weixinQr" :src="`data:image/png;base64,${weixinQr}`" alt="Weixin QR" class="w-40 h-40" />
    </div>

    <div class="rounded-xl border border-border p-4 space-y-2">
      <h3 class="text-sm font-medium">配对审批</h3>
      <p class="text-xs text-muted">在 IM 收到配对码后，在此输入并批准。</p>
      <div class="flex flex-wrap gap-2">
        <input v-model="pairingCode" placeholder="配对码" class="field flex-1 min-w-[8rem]" />
        <button type="button" class="btn-primary" @click="approvePairingAuto">自动批准</button>
        <button type="button" class="btn-ghost" @click="approvePairing('weixin')">微信批准</button>
        <button type="button" class="btn-ghost" @click="approvePairing('wecom')">企微批准</button>
        <button type="button" class="btn-ghost" @click="approvePairing('feishu')">飞书批准</button>
        <button type="button" class="btn-ghost" @click="approvePairing('dingtalk')">钉钉批准</button>
      </div>
    </div>

    <div class="flex gap-2">
      <button type="button" class="btn-primary" :disabled="saving" @click="save">
        {{ saving ? '保存中…' : '保存通道配置' }}
      </button>
      <button type="button" class="btn-ghost" :disabled="loading" @click="refresh">
        <RefreshCw class="w-3.5 h-3.5" />
        刷新
      </button>
    </div>
  </div>
</template>

<style scoped>
.field {
  @apply w-full rounded-lg border border-border bg-background px-3 py-2 text-sm;
}
.btn-ghost {
  @apply inline-flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-hover;
}
.btn-primary {
  @apply rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50;
}
</style>
