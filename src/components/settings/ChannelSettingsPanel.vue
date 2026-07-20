<script setup lang="ts">
import { ChevronDown, CircleHelp, Copy, Plug, QrCode, RefreshCw } from 'lucide-vue-next'
import { useChannelSettingsForm } from '../../composables/useChannelSettingsForm'

const {
  loading,
  error,
  pairingSuccess,
  pairingCode,
  CHANNEL_TABS,
  activeTab,
  config,
  channelStatusByTab,
  activeConnectionLabel,
  tabHints,
  activeQrBase64,
  activeStatus,
  statusToneClass,
  activeScanBusy,
  tauriMode,
  activeScanLabel,
  connectionByTab,
  activeConnectDisabled,
  activeConnectLabel,
  connectActiveChannel,
  startActiveScan,
  webhookUrls,
  urlKey,
  copyWebhook,
  COMMON_SETTINGS_HELP,
  IM_OUTBOUND_HELP,
  sendIntermediateText,
  sendToolCalls,
  SESSION_RESET_HELP,
  idleMinutes,
  setIdleMinutes,
  PUBLIC_BASE_URL_HELP,
  PAIRING_HELP,
  approvePairingAuto,
  refresh,
  save
} = useChannelSettingsForm()

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
                <span>工具调用</span>
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
