<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

import { computed, onMounted, ref } from 'vue'
import { Clock, Plus, Trash2, Webhook, ShieldCheck, AlertTriangle, AlertCircle, RefreshCw, MessagesSquare, CircleHelp, X, Copy, Check, Pencil } from 'lucide-vue-next'
import { useChatStore } from '../../../stores/chat'
import {
  listCronJobs,
  listCronDeliveryTargets,
  createCronJob,
  updateCronJob,
  deleteCronJob,
  listAgents,
  getWebhookConfig,
  setWebhookSourceToken,
  patchWebhookSource,
  clearWebhookSourceToken,
  clearWebhookLegacyToken,
  revealWebhookSourceToken
} from '../../../lib/api'
import { isTauriRuntime } from '../../../lib/runtime'
import { composerAgentLabel, composerAgentLabelById, resolveAgentUi } from '../../../lib/agentUi'
import { sortComposerAgents } from '../../../lib/agentIcons'
import { describeCronJob, resolveCronViewSessionId } from '../../../lib/cronSchedule'
import { resolveWebhookViewSessionId } from '../../../lib/webhookIngress'
import { WEBHOOK_URL_TEMPLATE, generateWebhookToken, webhookIngressCurl, webhookIngressUrl } from '../../../lib/webhookIngress'
import { DEFAULT_LEAD_AGENT_ID } from '../../../types/chat'
import type { AgentDef } from '../../../types/chat'
import type {
  CronJob,
  CreateCronJobInput,
  CronDeliveryTarget,
  WebhookConfig,
  WebhookSource,
  WebhookSessionMode
} from '../../../types/automation'
import CronSchedulePicker from './CronSchedulePicker.vue'

const emit = defineEmits<{ (e: 'view-session'): void }>()

const chat = useChatStore()

const jobs = ref<CronJob[]>([])
const loadingJobs = ref(false)
const jobsError = ref<string | null>(null)
const deliveryTargets = ref<CronDeliveryTarget[]>([])

const webhook = ref<WebhookConfig | null>(null)
const loadingWebhook = ref(false)
const webhookError = ref<string | null>(null)
const webhookInfo = ref<string | null>(null)
const webhookSrcInput = ref('')
const tokenInput = ref('')
const authHeaderInput = ref('')
const sessionModeInput = ref<WebhookSessionMode>('per_delivery')
const updatingSessionModeSrc = ref<string | null>(null)
const settingToken = ref(false)
const showWebhookForm = ref(false)
const webhookFormError = ref<string | null>(null)
const clearingLegacy = ref(false)
const pendingDeleteWebhookSrc = ref<string | null>(null)
const clearingWebhookSrc = ref<string | null>(null)
const pendingDeleteJobId = ref<string | null>(null)
const copiedWebhookCurlSrc = ref<string | null>(null)
const copiedWebhookTokenSrc = ref<string | null>(null)
const revealingWebhookTokenSrc = ref<string | null>(null)

// Agent list for the single "agent" picker (mirrors the composer: enabled
// user-selectable workers, Chinese label, default 通用助手).
const agents = ref<AgentDef[]>([])
const agentOptions = computed(() => {
  const list = agents.value.filter(
    a => a.role === 'worker' && a.enabled && resolveAgentUi(a).userSelectable
  )
  return sortComposerAgents(list).map(a => ({ id: a.id, label: composerAgentLabel(a) }))
})

// Create-form state. No target-session picker: each cron job owns a dedicated
// isolated `cron:{id}` session (OpenClaw model), created lazily on first fire.
const showForm = ref(false)
const form = ref({
  id: '',
  label: '',
  cronExpr: '0 0 9 * * *',
  promptText: '',
  agentId: DEFAULT_LEAD_AGENT_ID,
  enabled: true,
  pushIm: false,
  deliverChannels: [] as string[]
})
const creating = ref(false)
const formError = ref<string | null>(null)

/** Inline edit of deliver for an existing job. */
const editingDeliverJobId = ref<string | null>(null)
const editPushIm = ref(false)
const editDeliverChannels = ref<string[]>([])
const savingDeliver = ref(false)

const isDesktop = isTauriRuntime()

const CRON_SECTION_DESC = computed(() => t('settings.automation.cronDesc'))
const WEBHOOK_SECTION_DESC = computed(() => t('settings.automation.webhookDesc'))
const DELIVER_HINT = computed(() => t('settings.automation.deliverHint'))
const WEBHOOK_REF_BLOCKING = computed(() => t('settings.automation.blockingHint'))
const SESSION_MODE_HINT = computed(() => t('settings.automation.sessionModeHint'))
const LEGACY_TOKEN_DESC = computed(() => t('settings.automation.legacyTokenDesc'))
const AUTH_HEADER_HINT = computed(() => t('settings.automation.authHeaderHint'))
const TOKEN_HINT = computed(() => t('settings.automation.tokenHint'))
const WEBHOOK_TOKEN_COPY_UNAVAILABLE = computed(() => t('settings.automation.tokenUnavailable'))

function fmtMs(ms?: number | null): string {
  if (!ms) return '—'
  return new Date(ms).toLocaleString()
}

function genId(): string {
  return 'cron-' + Math.random().toString(36).slice(2, 8) + Date.now().toString(36).slice(-4)
}

const CHANNEL_LABEL = computed((): Record<string, string> => ({
  feishu: t('settings.channels.brands.feishu'),
  dingtalk: t('settings.channels.brands.dingtalk'),
  wecom: t('settings.automation.brandWecom'),
  weixin: t('settings.channels.brands.weixin')
}))

function channelLabel(channel: string): string {
  return CHANNEL_LABEL.value[channel] || channel
}

/** Bound channels first; used by create/edit multi-select. */
const boundDeliveryTargets = computed(() =>
  deliveryTargets.value.filter(t => t.bound)
)

function defaultDeliverChannels(): string[] {
  const bound = boundDeliveryTargets.value.map(t => t.channel)
  if (bound.length === 1) return [...bound]
  return []
}

function deliverToChannels(deliver: string | null | undefined): string[] {
  const d = (deliver ?? '').trim()
  if (!d) return []
  if (d === 'all') {
    return boundDeliveryTargets.value.map(t => t.channel)
  }
  return d
    .split(',')
    .map(s => s.trim().toLowerCase())
    .filter(ch => ['feishu', 'dingtalk', 'wecom', 'weixin'].includes(ch))
}

function channelsToDeliver(pushIm: boolean, channels: string[]): string | null {
  if (!pushIm) return null
  const unique = [...new Set(channels.map(c => c.trim().toLowerCase()).filter(Boolean))]
  if (unique.length === 0) return null
  const boundSet = new Set(boundDeliveryTargets.value.map(t => t.channel))
  if (unique.length === boundSet.size && unique.every(c => boundSet.has(c)) && boundSet.size > 1) {
    return 'all'
  }
  return unique.join(',')
}

function formatDeliverLabel(deliver: string | null | undefined): string {
  const channels = deliverToChannels(deliver)
  if (!channels.length) {
    const d = (deliver ?? '').trim()
    return d || ''
  }
  return channels.map(channelLabel).join('、')
}

function toggleFormChannel(channel: string, checked: boolean) {
  const set = new Set(form.value.deliverChannels)
  if (checked) set.add(channel)
  else set.delete(channel)
  form.value.deliverChannels = [...set]
}

function toggleEditChannel(channel: string, checked: boolean) {
  const set = new Set(editDeliverChannels.value)
  if (checked) set.add(channel)
  else set.delete(channel)
  editDeliverChannels.value = [...set]
}

async function refreshJobs() {
  loadingJobs.value = true
  jobsError.value = null
  try {
    jobs.value = await listCronJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
  } finally {
    loadingJobs.value = false
  }
}

async function refreshDeliveryTargets() {
  try {
    deliveryTargets.value = await listCronDeliveryTargets()
  } catch (e) {
    console.warn('[automation] listCronDeliveryTargets failed', e)
    deliveryTargets.value = []
  }
}

async function refreshWebhook() {
  loadingWebhook.value = true
  webhookError.value = null
  try {
    webhook.value = await getWebhookConfig()
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    loadingWebhook.value = false
  }
}

async function loadAgentsList() {
  try {
    agents.value = await listAgents()
  } catch (e) {
    console.warn('[automation] loadAgents failed', e)
  }
}

function openCreateForm() {
  form.value = {
    id: genId(),
    label: '',
    cronExpr: '0 * * * * *',
    promptText: '',
    agentId: DEFAULT_LEAD_AGENT_ID,
    enabled: true,
    pushIm: false,
    deliverChannels: []
  }
  formError.value = null
  showForm.value = true
  editingDeliverJobId.value = null
}

async function submitCreate() {
  if (!form.value.label.trim()) { formError.value = t('settings.automation.nameRequired'); return }
  if (!form.value.cronExpr.trim()) { formError.value = t('settings.automation.cronRequired'); return }
  if (!form.value.promptText.trim()) { formError.value = t('settings.automation.promptRequired'); return }
  if (form.value.pushIm && form.value.deliverChannels.length === 0) {
    formError.value = t('settings.automation.channelRequired')
    return
  }
  creating.value = true
  formError.value = null
  try {
    const schedule = form.value.cronExpr.trim()
    const input: CreateCronJobInput = {
      id: form.value.id,
      label: form.value.label.trim(),
      schedule,
      cronExpr: schedule,
      promptText: form.value.promptText.trim(),
      leadAgentId: form.value.agentId || DEFAULT_LEAD_AGENT_ID,
      enabled: form.value.enabled,
      deliver: channelsToDeliver(form.value.pushIm, form.value.deliverChannels)
    }
    await createCronJob(input)
    showForm.value = false
    await refreshJobs()
  } catch (e) {
    formError.value = (e as Error).message
  } finally {
    creating.value = false
  }
}

async function toggleEnabled(job: CronJob, enabled: boolean) {
  if (enabled && job.scheduleKind === 'once') {
    jobsError.value = t('settings.automation.onceNoReenable')
    return
  }
  try {
    await updateCronJob(job.id, { enabled })
    await refreshJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
    await refreshJobs()
  }
}

function openEditDeliver(job: CronJob) {
  showForm.value = false
  const channels = deliverToChannels(job.deliver)
  editingDeliverJobId.value = job.id
  editPushIm.value = channels.length > 0 || !!(job.deliver && job.deliver.trim())
  editDeliverChannels.value = channels.length
    ? channels
    : defaultDeliverChannels()
}

function cancelEditDeliver() {
  editingDeliverJobId.value = null
  editPushIm.value = false
  editDeliverChannels.value = []
}

async function saveEditDeliver(job: CronJob) {
  if (editPushIm.value && editDeliverChannels.value.length === 0) {
    jobsError.value = t('settings.automation.channelRequired')
    return
  }
  savingDeliver.value = true
  jobsError.value = null
  try {
    const deliver = channelsToDeliver(editPushIm.value, editDeliverChannels.value)
    await updateCronJob(job.id, { deliver: deliver ?? '' })
    cancelEditDeliver()
    await refreshJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
  } finally {
    savingDeliver.value = false
  }
}

async function removeJob(job: CronJob) {
  if (pendingDeleteJobId.value !== job.id) {
    pendingDeleteJobId.value = job.id
    return
  }
  pendingDeleteJobId.value = null
  try {
    await deleteCronJob(job.id)
    await refreshJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
  }
}

function cancelDeleteJob() {
  pendingDeleteJobId.value = null
}

function cronViewSessionId(job: CronJob): string | null {
  return resolveCronViewSessionId(job)
}

function cronAgentLabel(job: CronJob): string {
  return composerAgentLabelById(job.leadAgentId, agents.value)
}

function cronJobTitle(job: CronJob): string {
  const deliver = formatDeliverLabel(job.deliver)
  const deliverPart = deliver ? t('settings.automation.deliverSep', { label: deliver }) : ''
  const err = job.lastDeliveryError
    ? t('settings.automation.deliveryFailSep', { error: job.lastDeliveryError })
    : ''
  return t('settings.automation.jobTitleFull', {
    label: job.label,
    agent: cronAgentLabel(job),
    schedule: describeCronJob(job),
    next: fmtMs(job.nextRunAtMs),
    last: fmtMs(job.lastRunAtMs),
    deliver: deliverPart,
    error: err
  })
}

// Open the cron job's active isolated session in the main panel.
function viewSession(job: CronJob) {
  const sessionId = cronViewSessionId(job)
  if (!sessionId) {
    jobsError.value = t('settings.automation.noSessionJob')
    return
  }
  jobsError.value = null
  chat.openCronConversation(sessionId, job.label, job.leadAgentId)
  // Close the settings dialog so the user actually sees the conversation they
  // just opened — otherwise the dialog stays on top and the click appears to
  // do nothing.
  emit('view-session')
}

async function submitWebhookSource() {
  const src = webhookSrcInput.value.trim()
  if (!src) { webhookFormError.value = t('settings.automation.sourceIdRequired'); return }
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(src)) {
    webhookFormError.value = t('settings.automation.sourceIdInvalid')
    return
  }
  if (!tokenInput.value.trim()) { webhookFormError.value = t('settings.automation.tokenRequired'); return }
  const token = tokenInput.value.trim()
  const authHeaderName = authHeaderInput.value.trim() || null
  settingToken.value = true
  webhookFormError.value = null
  webhookError.value = null
  webhookInfo.value = null
  try {
    webhook.value = await setWebhookSourceToken(src, token, authHeaderName, sessionModeInput.value)
    if (!isDesktop) {
      webhookInfo.value = t('settings.automation.tokenCopied')
      navigator.clipboard.writeText(token).catch(e => {
        console.warn('[automation] copy webhook token after create failed', e)
        webhookInfo.value = t('settings.automation.sourceAdded')
      })
    }
    webhookSrcInput.value = ''
    tokenInput.value = ''
    authHeaderInput.value = ''
    sessionModeInput.value = 'per_delivery'
    showWebhookForm.value = false
    await refreshWebhook()
  } catch (e) {
    webhookFormError.value = (e as Error).message
  } finally {
    settingToken.value = false
  }
}

function openWebhookCreateForm() {
  webhookSrcInput.value = ''
  tokenInput.value = generateWebhookToken()
  authHeaderInput.value = ''
  sessionModeInput.value = 'daily'
  webhookFormError.value = null
  showWebhookForm.value = true
}

async function removeWebhookSource(src: string) {
  if (pendingDeleteWebhookSrc.value !== src) {
    pendingDeleteWebhookSrc.value = src
    return
  }
  pendingDeleteWebhookSrc.value = null
  clearingWebhookSrc.value = src
  webhookError.value = null
  try {
    await clearWebhookSourceToken(src)
    await refreshWebhook()
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    clearingWebhookSrc.value = null
  }
}

function cancelDeleteWebhookSource() {
  pendingDeleteWebhookSrc.value = null
}

async function updateWebhookSessionMode(source: WebhookSource, mode: WebhookSessionMode) {
  if (source.sessionMode === mode) return
  updatingSessionModeSrc.value = source.src
  webhookError.value = null
  try {
    webhook.value = await patchWebhookSource(source.src, { sessionMode: mode })
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    updatingSessionModeSrc.value = null
  }
}

function webhookSessionModeLabel(mode?: WebhookSessionMode): string {
  return mode === 'per_delivery'
    ? t('settings.automation.modePerDelivery')
    : t('settings.automation.modeDaily')
}

function webhookViewSessionId(source: WebhookSource): string | null {
  return resolveWebhookViewSessionId(source)
}

function viewWebhookSession(source: WebhookSource) {
  const sessionId = webhookViewSessionId(source)
  if (!sessionId) {
    webhookError.value = t('settings.automation.noSessionSrcDetail')
    return
  }
  webhookError.value = null
  chat.openWebhookConversation(sessionId, source.src)
  emit('view-session')
}

async function resolveWebhookToken(source: WebhookSource): Promise<string | null> {
  const cached = source.token?.trim()
  if (cached) return cached
  revealingWebhookTokenSrc.value = source.src
  webhookError.value = null
  try {
    const revealed = await revealWebhookSourceToken(source.src)
    return revealed.token?.trim() || null
  } catch (e) {
    webhookError.value = (e as Error).message || WEBHOOK_TOKEN_COPY_UNAVAILABLE.value
    return null
  } finally {
    if (revealingWebhookTokenSrc.value === source.src) {
      revealingWebhookTokenSrc.value = null
    }
  }
}

async function copyWebhookCurl(source: WebhookSource) {
  const token = await resolveWebhookToken(source)
  if (!token) {
    if (!webhookError.value) webhookError.value = WEBHOOK_TOKEN_COPY_UNAVAILABLE.value
    return
  }
  webhookError.value = null
  const text = webhookIngressCurl(source.src, token, source.authHeaderName)
  navigator.clipboard.writeText(text).then(() => {
    copiedWebhookCurlSrc.value = source.src
    setTimeout(() => {
      if (copiedWebhookCurlSrc.value === source.src) copiedWebhookCurlSrc.value = null
    }, 2000)
  }).catch(e => console.warn('[automation] copyWebhookCurl failed', e))
}

async function copyWebhookToken(source: WebhookSource) {
  const token = await resolveWebhookToken(source)
  if (!token) {
    if (!webhookError.value) webhookError.value = WEBHOOK_TOKEN_COPY_UNAVAILABLE.value
    return
  }
  webhookError.value = null
  navigator.clipboard.writeText(token).then(() => {
    copiedWebhookTokenSrc.value = source.src
    setTimeout(() => {
      if (copiedWebhookTokenSrc.value === source.src) copiedWebhookTokenSrc.value = null
    }, 2000)
  }).catch(e => console.warn('[automation] copyWebhookToken failed', e))
}

async function clearLegacyToken() {
  clearingLegacy.value = true
  webhookError.value = null
  try {
    await clearWebhookLegacyToken()
    await refreshWebhook()
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    clearingLegacy.value = false
  }
}

onMounted(() => {
  refreshJobs()
  refreshDeliveryTargets()
  loadAgentsList()
  // Webhook 仅 web/server 端可用；桌面端无 HTTP 入口，跳过状态加载。
  if (!isDesktop) refreshWebhook()
})
</script>

<template>
  <div class="space-y-5">
    <!-- ============ Cron jobs ============ -->
    <section class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-3 mb-3">
        <div class="flex items-center gap-2 min-w-0">
          <Clock class="w-4 h-4 text-accent shrink-0" />
          <span class="text-sm font-medium text-foreground whitespace-nowrap">{{ t('settings.automation.cronJobs') }}</span>
          <button
            type="button"
            class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
            :title="CRON_SECTION_DESC"
            :aria-label="t('settings.s_2a438d')"
            @click.stop
          >
            <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
          </button>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
            :title="t('workspace.refresh')"
            :disabled="loadingJobs"
            @click="refreshJobs"
          >
            <RefreshCw class="w-3.5 h-3.5" :class="loadingJobs ? 'animate-spin text-muted' : 'text-muted'" />
          </button>
          <button
            class="h-7 px-3 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 inline-flex items-center gap-1 cursor-pointer whitespace-nowrap shrink-0"
            @click="openCreateForm"
          >
            <Plus class="w-3.5 h-3.5 shrink-0" />{{ t('settings.automation.new') }}
          </button>
        </div>
      </div>

      <p v-if="jobsError" class="text-xs text-danger mb-2">{{ jobsError }}</p>

      <div v-if="!loadingJobs && jobs.length === 0" class="text-xs text-muted py-4 text-center">
        {{ t('settings.automation.empty') }}
      </div>

      <div v-else class="space-y-2">
        <div
          v-for="job in jobs"
          :key="job.id"
          class="rounded-lg border border-border bg-card/40 px-3 py-2 min-w-0 space-y-1.5"
        >
          <div class="flex items-center gap-2 min-w-0">
            <button
              class="relative h-5 w-9 rounded-full transition-colors shrink-0 cursor-pointer"
              :class="job.enabled ? 'bg-accent' : 'bg-hover'"
              :title="job.enabled ? t('settings.automation.enabledClickDisable') : t('settings.automation.disabledClickEnable')"
              @click="toggleEnabled(job, !job.enabled)"
            >
              <span
                class="absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all"
                :class="job.enabled ? 'left-[18px]' : 'left-0.5'"
              />
            </button>
            <div
              class="min-w-0 flex-1 flex items-center gap-1.5 text-[12px] truncate"
              :title="cronJobTitle(job)"
            >
              <span class="font-medium text-foreground truncate">{{ job.label }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="text-muted truncate">{{ cronAgentLabel(job) }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="text-muted truncate">{{ describeCronJob(job) }}</span>
              <template v-if="formatDeliverLabel(job.deliver)">
                <span class="text-muted/50 shrink-0">·</span>
                <span class="text-accent/80 truncate text-[11px]" :title="t('settings.automation.deliverPrefix', { label: formatDeliverLabel(job.deliver) })">{{ formatDeliverLabel(job.deliver) }}</span>
              </template>
            </div>
            <div
              class="text-[11px] text-muted shrink-0 whitespace-nowrap"
              :title="t('settings.automation.nextLast', { next: fmtMs(job.nextRunAtMs), last: fmtMs(job.lastRunAtMs) })"
            >
              {{ t('settings.automation.nextRunShort', { when: fmtMs(job.nextRunAtMs) }) }}
            </div>
            <div class="flex items-center gap-1 shrink-0">
              <button
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center text-muted cursor-pointer shrink-0"
                :title="job.deliver ? t('settings.automation.editDelivery') : t('settings.automation.setDelivery')"
                @click="openEditDeliver(job)"
              >
                <Pencil class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md inline-flex items-center justify-center shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
                :class="cronViewSessionId(job)
                  ? 'border border-border bg-hover text-foreground hover:bg-hover cursor-pointer'
                  : 'border border-border text-muted cursor-not-allowed'"
                :title="cronViewSessionId(job) ? t('settings.automation.viewSession') : t('settings.automation.noSessionYet')"
                :disabled="!cronViewSessionId(job)"
                @click="viewSession(job)"
              >
                <MessagesSquare class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md border inline-flex items-center justify-center shrink-0 cursor-pointer"
                :class="pendingDeleteJobId === job.id
                  ? 'border-danger/40 bg-danger/10 text-danger hover:bg-danger/15'
                  : 'border-border hover:bg-hover text-muted'"
                :title="pendingDeleteJobId === job.id ? t('settings.automation.confirmDelete') : t('common.delete')"
                @click="removeJob(job)"
              >
                <Trash2 class="w-3.5 h-3.5" />
              </button>
              <button
                v-if="pendingDeleteJobId === job.id"
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center text-muted cursor-pointer shrink-0"
                :title="t('common.cancel')"
                @click="cancelDeleteJob"
              >
                <X class="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <p
            v-if="job.lastDeliveryError"
            class="text-[11px] text-warning truncate pl-11"
            :title="job.lastDeliveryError"
          >
            {{ t('settings.automation.deliveryFail', { error: job.lastDeliveryError }) }}
          </p>
          <div
            v-if="editingDeliverJobId === job.id"
            class="pl-11 pr-1 pb-1 space-y-2"
          >
            <label class="flex items-center gap-2 text-[12px] text-foreground cursor-pointer">
              <input v-model="editPushIm" type="checkbox" class="rounded border-border" />
              {{ t('settings.automation.pushIm') }}
              <span
                class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help"
                :title="DELIVER_HINT"
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </span>
            </label>
            <div v-if="editPushIm" class="space-y-1.5">
              <label
                v-for="dt in deliveryTargets"
                :key="dt.channel"
                class="flex items-center gap-2 text-[12px]"
                :class="dt.bound ? 'text-foreground cursor-pointer' : 'text-muted cursor-not-allowed'"
              >
                <input
                  type="checkbox"
                  class="rounded border-border"
                  :disabled="!dt.bound"
                  :checked="editDeliverChannels.includes(dt.channel)"
                  @change="toggleEditChannel(dt.channel, ($event.target as HTMLInputElement).checked)"
                />
                <span>{{ dt.label }}</span>
              </label>
              <p v-if="deliveryTargets.length === 0" class="text-[11px] text-muted">
                {{ t('settings.automation.noImChannel') }}
              </p>
              <p v-else-if="boundDeliveryTargets.length === 0" class="text-[11px] text-muted">
                {{ t('settings.automation.bindImFirst') }}
              </p>
            </div>
            <div class="flex items-center justify-end gap-2">
              <button
                class="h-7 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer"
                @click="cancelEditDeliver"
              >
                {{ t('settings.automation.cancel') }}
              </button>
              <button
                class="h-7 px-3 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
                :disabled="savingDeliver"
                @click="saveEditDeliver(job)"
              >
                {{ savingDeliver ? t('settings.automation.saving') : t('common.save') }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- Create form -->
      <div v-if="showForm" class="mt-4 rounded-lg border border-accent/30 bg-accent/5 p-4 space-y-3">
        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="text-[11px] text-muted">{{ t('settings.automation.name') }}</span>
            <input v-model="form.label" class="input-base mt-1" :placeholder="t('settings.automation.namePlaceholder')" />
          </label>
          <label class="block">
            <span class="text-[11px] text-muted">{{ t('settings.automation.agent') }}</span>
            <select v-model="form.agentId" class="input-base mt-1">
              <option v-for="a in agentOptions" :key="a.id" :value="a.id">{{ a.label }}</option>
            </select>
          </label>
        </div>
        <label class="block">
          <span class="text-[11px] text-muted">{{ t('settings.automation.prompt') }}</span>
          <textarea v-model="form.promptText" rows="3" class="input-base mt-1 resize-y" :placeholder="t('settings.automation.promptPlaceholder')" />
        </label>
        <CronSchedulePicker v-model="form.cronExpr" />
        <div class="space-y-2">
          <label class="flex items-center gap-2 text-[12px] text-foreground cursor-pointer">
            <input v-model="form.pushIm" type="checkbox" class="rounded border-border" />
            {{ t('settings.automation.pushIm') }}
            <span
              class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help"
              :title="DELIVER_HINT"
            >
              <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
            </span>
          </label>
          <div v-if="form.pushIm" class="space-y-1.5 pl-0.5">
            <label
              v-for="dt in deliveryTargets"
              :key="dt.channel"
              class="flex items-center gap-2 text-[12px]"
              :class="dt.bound ? 'text-foreground cursor-pointer' : 'text-muted cursor-not-allowed'"
            >
              <input
                type="checkbox"
                class="rounded border-border"
                :disabled="!dt.bound"
                :checked="form.deliverChannels.includes(dt.channel)"
                @change="toggleFormChannel(dt.channel, ($event.target as HTMLInputElement).checked)"
              />
              <span>{{ dt.label }}</span>
            </label>
            <p v-if="deliveryTargets.length === 0" class="text-[11px] text-muted">
              {{ t('settings.automation.noImChannel') }}
            </p>
            <p v-else-if="boundDeliveryTargets.length === 0" class="text-[11px] text-muted">
              {{ t('settings.automation.bindImFirst') }}
            </p>
          </div>
        </div>
        <p v-if="formError" class="text-xs text-danger">{{ formError }}</p>
        <div class="flex items-center justify-end gap-2">
          <button class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer" @click="showForm = false">{{ t('settings.automation.cancel') }}</button>
          <button
            class="h-8 px-4 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
            :disabled="creating"
            @click="submitCreate"
          >
            {{ creating ? t('settings.automation.creating') : t('settings.automation.create') }}
          </button>
        </div>
      </div>
    </section>

    <!-- ============ Webhook (web/server only) ============ -->
    <section v-if="!isDesktop" class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-3 mb-3">
        <div class="flex items-center gap-2 min-w-0">
          <Webhook class="w-4 h-4 text-accent shrink-0" />
          <span class="text-sm font-medium text-foreground whitespace-nowrap">{{ t('settings.automation.webhook') }}</span>
          <button
            type="button"
            class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
            :title="WEBHOOK_SECTION_DESC"
            :aria-label="t('settings.automation.webhookHelp')"
            @click.stop
          >
            <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
          </button>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
            :title="t('settings.automation.refresh')"
            :disabled="loadingWebhook"
            @click="refreshWebhook"
          >
            <RefreshCw class="w-3.5 h-3.5" :class="loadingWebhook ? 'animate-spin text-muted' : 'text-muted'" />
          </button>
          <button
            class="h-7 px-3 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 inline-flex items-center gap-1 cursor-pointer whitespace-nowrap shrink-0"
            @click="openWebhookCreateForm"
          >
            <Plus class="w-3.5 h-3.5 shrink-0" />{{ t('settings.automation.new') }}
          </button>
        </div>
      </div>

      <p v-if="webhookInfo" class="text-xs text-success mb-2">{{ webhookInfo }}</p>
      <p v-if="webhookError" class="text-xs text-danger mb-2">{{ webhookError }}</p>

      <div v-if="webhook" class="space-y-3">
        <div
          v-if="webhook.legacyConfigured"
          class="flex items-center gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs"
        >
          <AlertTriangle class="w-3.5 h-3.5 text-warning shrink-0" />
          <span
            class="min-w-0 flex-1 truncate text-foreground"
            :title="t('settings.automation.legacyTokenBanner', { preview: webhook.legacyPreview, desc: LEGACY_TOKEN_DESC })"
          >
            {{ t('settings.automation.legacyTokenBanner', { preview: webhook.legacyPreview, desc: LEGACY_TOKEN_DESC }) }}
          </span>
          <button
            class="h-7 px-2 rounded border border-border hover:bg-hover text-xs cursor-pointer disabled:opacity-50 whitespace-nowrap shrink-0"
            :disabled="clearingLegacy"
            @click="clearLegacyToken"
          >
            {{ clearingLegacy ? t('settings.automation.clearing') : t('settings.automation.clearLegacyToken') }}
          </button>
        </div>

        <div v-if="!loadingWebhook && webhook.sources.length === 0" class="text-xs text-muted py-4 text-center">
          {{ t('settings.automation.noSources') }}
        </div>

        <div v-else-if="webhook.sources.length" class="space-y-2">
          <div
            v-for="s in webhook.sources"
            :key="s.src"
            class="flex items-center gap-2 rounded-lg border border-border bg-card/40 px-3 py-2 min-w-0"
          >
            <ShieldCheck class="w-3.5 h-3.5 text-success shrink-0" />
            <div
              class="min-w-0 flex-1 flex items-center gap-1.5 text-[12px] truncate"
              :title="`${s.src} · ${s.preview}${s.authHeaderName ? ` · ${s.authHeaderName}` : ''} · ${webhookIngressUrl(s.src)}`"
            >
              <span class="font-medium text-foreground whitespace-nowrap shrink-0">{{ s.src }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <button
                type="button"
                class="font-mono text-muted truncate hover:text-foreground cursor-pointer text-left min-w-0 disabled:opacity-50"
                :disabled="revealingWebhookTokenSrc === s.src"
                :title="revealingWebhookTokenSrc === s.src
                  ? t('settings.automation.fetchingToken')
                  : (copiedWebhookTokenSrc === s.src ? t('settings.automation.tokenCopiedShort') : t('settings.automation.copyToken'))"
                @click="copyWebhookToken(s)"
              >
                <span v-if="copiedWebhookTokenSrc === s.src" class="text-success">{{ t('settings.automation.copied') }}</span>
                <span v-else-if="revealingWebhookTokenSrc === s.src" class="text-muted">{{ t('settings.automation.copying') }}</span>
                <span v-else>{{ s.preview }}</span>
              </button>
              <span v-if="s.authHeaderName" class="text-muted/50 shrink-0">·</span>
              <span v-if="s.authHeaderName" class="font-mono text-[10px] text-muted/80 truncate">{{ s.authHeaderName }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="font-mono text-[10px] text-muted/70 truncate">{{ webhookIngressUrl(s.src) }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="text-[10px] text-muted/80 whitespace-nowrap shrink-0">{{ webhookSessionModeLabel(s.sessionMode) }}</span>
            </div>
            <div class="flex items-center gap-1 shrink-0">
              <select
                class="h-7 max-w-[7.5rem] rounded-md border border-border bg-card px-1.5 text-[10px] text-muted cursor-pointer disabled:opacity-50"
                :value="s.sessionMode ?? 'per_delivery'"
                :disabled="updatingSessionModeSrc === s.src"
                :title="SESSION_MODE_HINT"
                @change="updateWebhookSessionMode(s, ($event.target as HTMLSelectElement).value as WebhookSessionMode)"
              >
                <option value="daily">{{ t('settings.automation.modeDaily') }}</option>
                <option value="per_delivery">{{ t('settings.automation.modePerDelivery') }}</option>
              </select>
              <button
                class="h-7 w-7 rounded-md inline-flex items-center justify-center shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
                :class="webhookViewSessionId(s)
                  ? 'border border-border bg-hover text-foreground hover:bg-hover cursor-pointer'
                  : 'border border-border text-muted cursor-not-allowed'"
                :title="webhookViewSessionId(s) ? t('settings.automation.viewSessionSrc') : t('settings.automation.noSessionSrc')"
                :disabled="!webhookViewSessionId(s)"
                @click="viewWebhookSession(s)"
              >
                <MessagesSquare class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center shrink-0 cursor-pointer text-muted disabled:opacity-40 disabled:cursor-not-allowed"
                :disabled="revealingWebhookTokenSrc === s.src"
                :title="revealingWebhookTokenSrc === s.src ? t('settings.automation.fetchingToken') : t('settings.automation.copyCurl')"
                @click="copyWebhookCurl(s)"
              >
                <Check v-if="copiedWebhookCurlSrc === s.src" class="w-3.5 h-3.5 text-success" />
                <Copy v-else class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md border inline-flex items-center justify-center shrink-0 cursor-pointer disabled:opacity-50"
                :class="pendingDeleteWebhookSrc === s.src
                  ? 'border-danger/40 bg-danger/10 text-danger hover:bg-danger/15'
                  : 'border-border hover:bg-hover text-muted'"
                :title="pendingDeleteWebhookSrc === s.src ? t('settings.automation.confirmDeleteSource') : t('settings.automation.deleteSource')"
                :disabled="clearingWebhookSrc === s.src"
                @click="removeWebhookSource(s.src)"
              >
                <Trash2 class="w-3.5 h-3.5" />
              </button>
              <button
                v-if="pendingDeleteWebhookSrc === s.src"
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center text-muted cursor-pointer shrink-0"
                :title="t('common.cancel')"
                @click="cancelDeleteWebhookSource"
              >
                <X class="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
        </div>

        <!-- Create form -->
        <div v-if="showWebhookForm" class="rounded-lg border border-accent/30 bg-accent/5 p-4 space-y-3">
          <div class="flex items-end gap-4 min-w-0">
            <label class="block shrink-0 space-y-2">
              <div class="flex h-[14px] items-center">
                <span class="text-[11px] text-muted whitespace-nowrap">{{ t('settings.automation.sourceId') }}</span>
              </div>
              <input
                v-model="webhookSrcInput"
                class="input-base w-[20ch] max-w-[20ch] font-mono text-[12px]"
                placeholder="github"
                :disabled="settingToken"
              />
            </label>
            <label class="block flex-1 min-w-0 space-y-2">
              <div class="flex h-[14px] items-center gap-1">
                <span class="text-[11px] text-muted whitespace-nowrap">Token</span>
                <span
                  class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help shrink-0"
                  :title="TOKEN_HINT"
                >
                  <AlertCircle class="w-3.5 h-3.5 pointer-events-none" />
                </span>
              </div>
              <div class="flex items-center gap-2 min-w-0">
                <input
                  v-model="tokenInput"
                  type="password"
                  class="input-base flex-1 min-w-0 font-mono text-[12px]"
                  :placeholder="t('settings.automation.bearerOnce')"
                  autocomplete="new-password"
                  spellcheck="false"
                  :disabled="settingToken"
                />
                <button
                  type="button"
                  class="h-9 w-9 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center shrink-0 cursor-pointer text-muted"
                  :title="t('settings.automation.regenToken')"
                  :disabled="settingToken"
                  @click="tokenInput = generateWebhookToken()"
                >
                  <RefreshCw class="w-3.5 h-3.5" />
                </button>
              </div>
            </label>
            <label class="block shrink-0 space-y-2">
              <div class="flex h-[14px] items-center gap-1">
                <span class="text-[11px] text-muted whitespace-nowrap">{{ t('settings.automation.authHeader') }}</span>
                <span
                  class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help shrink-0"
                  :title="AUTH_HEADER_HINT"
                >
                  <AlertCircle class="w-3.5 h-3.5 pointer-events-none" />
                </span>
              </div>
              <input
                v-model="authHeaderInput"
                class="input-base w-[24ch] max-w-[24ch] font-mono text-[12px]"
                :placeholder="t('settings.automation.optional')"
                :disabled="settingToken"
              />
            </label>
            <label class="block shrink-0 space-y-2">
              <div class="flex h-[14px] items-center gap-1">
                <span class="text-[11px] text-muted whitespace-nowrap">{{ t('settings.automation.sessionMode') }}</span>
                <span
                  class="inline-flex items-center text-muted hover:text-foreground transition-colors cursor-help shrink-0"
                  :title="SESSION_MODE_HINT"
                >
                  <AlertCircle class="w-3.5 h-3.5 pointer-events-none" />
                </span>
              </div>
              <select
                v-model="sessionModeInput"
                class="input-base w-[9rem] text-[12px] cursor-pointer"
                :disabled="settingToken"
              >
                <option value="daily">{{ t('settings.automation.modeDaily') }}</option>
                <option value="per_delivery">{{ t('settings.automation.modePerDelivery') }}</option>
              </select>
            </label>
          </div>
          <p v-if="webhookFormError" class="text-xs text-danger">{{ webhookFormError }}</p>
          <div class="flex items-center justify-end gap-2 shrink-0">
            <button
              class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer"
              @click="showWebhookForm = false"
            >
              {{ t('common.cancel') }}
            </button>
            <button
              class="h-8 px-4 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
              :disabled="settingToken"
              @click="submitWebhookSource"
            >
              {{ settingToken ? t('settings.automation.adding') : t('settings.automation.add') }}
            </button>
          </div>
        </div>

        <div
          class="rounded-lg border border-border/60 bg-card/30 px-3 py-2.5 space-y-1.5 text-[11px] text-muted leading-relaxed"
        >
          <div class="flex items-start gap-1.5 min-w-0">
            <span class="shrink-0 whitespace-nowrap">{{ t('settings.automation.urlTemplate') }}</span>
            <code class="font-mono text-foreground break-all">{{ WEBHOOK_URL_TEMPLATE }}</code>
          </div>
          <div class="min-w-0">
            {{ t('settings.automation.authDefault') }}
            <code class="font-mono text-foreground/90">Authorization: Bearer …</code>
            {{ t('settings.automation.authOr') }}
            <code class="font-mono text-foreground/90">X-Pointer-Token</code>{{ t('settings.automation.authCustom') }}
          </div>
          <div class="min-w-0">
            {{ t('settings.automation.msgPrefer') }}
            <code class="font-mono text-foreground/90">text</code> /
            <code class="font-mono text-foreground/90">message</code>{{ t('settings.automation.msgFallback') }}
          </div>
          <div class="min-w-0" :title="WEBHOOK_REF_BLOCKING">
            {{ t('settings.automation.syncMode') }}
            <code class="font-mono text-foreground/90">"blocking": true</code>{{ t('settings.automation.syncOptional') }}
            <code class="font-mono text-foreground/90">"timeoutSeconds"</code>{{ t('settings.automation.syncRange') }}
          </div>
        </div>
      </div>
      <div v-else-if="loadingWebhook" class="text-xs text-muted">{{ t('settings.automation.loading') }}</div>
    </section>
  </div>
</template>

<style scoped>
.input-base {
  width: 100%;
  height: 2.25rem;
  border-radius: 0.5rem;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--card));
  color: hsl(var(--foreground));
  padding: 0 0.625rem;
  font-size: 0.8125rem;
  outline: none;
}
.input-base:focus {
  border-color: hsl(var(--accent));
}
textarea.input-base {
  height: auto;
  padding: 0.5rem 0.625rem;
  line-height: 1.4;
}
select.input-base {
  appearance: none;
  color-scheme: light dark;
  /* Custom chevron: appearance:none removes the native arrow, so render one
     ourselves so the control still reads as a dropdown. */
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23a3a3a3' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");
  background-repeat: no-repeat;
  background-position: right 0.5rem center;
  background-size: 0.875rem 0.875rem;
  padding-right: 1.75rem;
}
</style>
