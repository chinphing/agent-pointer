<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Clock, Plus, Trash2, Webhook, ShieldCheck, AlertTriangle, RefreshCw, MessagesSquare, CircleHelp, X, Copy, Check } from 'lucide-vue-next'
import { useChatStore } from '../../../stores/chat'
import {
  listCronJobs,
  createCronJob,
  updateCronJob,
  deleteCronJob,
  listAgents,
  getWebhookConfig,
  setWebhookSourceToken,
  clearWebhookSourceToken,
  clearWebhookLegacyToken
} from '../../../lib/api'
import { isTauriRuntime } from '../../../lib/runtime'
import { composerAgentLabel, composerAgentLabelById, resolveAgentUi } from '../../../lib/agentUi'
import { sortComposerAgents } from '../../../lib/agentIcons'
import { describeCron, resolveCronViewSessionId } from '../../../lib/cronSchedule'
import { resolveWebhookViewSessionId } from '../../../lib/webhookIngress'
import { WEBHOOK_URL_TEMPLATE, generateWebhookToken, recallWebhookToken, rememberWebhookToken, forgetWebhookToken, webhookIngressCurl, webhookIngressUrl } from '../../../lib/webhookIngress'
import { DEFAULT_LEAD_AGENT_ID } from '../../../types/chat'
import type { AgentDef } from '../../../types/chat'
import type { CronJob, CreateCronJobInput, WebhookConfig, WebhookSource } from '../../../types/automation'
import CronSchedulePicker from './CronSchedulePicker.vue'

const emit = defineEmits<{ (e: 'view-session'): void }>()

const chat = useChatStore()

const jobs = ref<CronJob[]>([])
const loadingJobs = ref(false)
const jobsError = ref<string | null>(null)

const webhook = ref<WebhookConfig | null>(null)
const loadingWebhook = ref(false)
const webhookError = ref<string | null>(null)
const webhookSrcInput = ref('')
const tokenInput = ref('')
const authHeaderInput = ref('')
const settingToken = ref(false)
const showWebhookForm = ref(false)
const webhookFormError = ref<string | null>(null)
const clearingLegacy = ref(false)
const pendingDeleteWebhookSrc = ref<string | null>(null)
const clearingWebhookSrc = ref<string | null>(null)
const pendingDeleteJobId = ref<string | null>(null)
const copiedWebhookCurlSrc = ref<string | null>(null)

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
  cronExpr: '0 * * * * *',
  promptText: '',
  agentId: DEFAULT_LEAD_AGENT_ID,
  enabled: true
})
const creating = ref(false)
const formError = ref<string | null>(null)

const isDesktop = isTauriRuntime()

const CRON_SECTION_DESC =
  '按 Cron 表达式定时触发，每个任务独占一个隔离会话，跨次续接上下文。触发后可点「查看会话」在主界面阅读 transcript。'
const WEBHOOK_SECTION_DESC =
  '每个来源独立 Token，须与 URL 路径中的来源标识匹配。POST 请求体支持 text 或 messages。触发后可点「查看会话」阅读 transcript。'
const WEBHOOK_REF_BLOCKING =
  'body 传 "blocking": true 时保持连接至 run 结束，返回 { ok, runId, text }；可选 "timeoutSeconds"（默认 120，最大 600）。未传时为 202 异步 ack。'
const LEGACY_TOKEN_DESC = '检测到旧版全局 Token，对所有来源生效。建议改为按来源配置。'

function fmtMs(ms?: number | null): string {
  if (!ms) return '—'
  return new Date(ms).toLocaleString()
}

function genId(): string {
  return 'cron-' + Math.random().toString(36).slice(2, 8) + Date.now().toString(36).slice(-4)
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
    enabled: true
  }
  formError.value = null
  showForm.value = true
}

async function submitCreate() {
  if (!form.value.label.trim()) { formError.value = '请填写名称'; return }
  if (!form.value.cronExpr.trim()) { formError.value = '请填写 Cron 表达式'; return }
  if (!form.value.promptText.trim()) { formError.value = '请填写触发提示词'; return }
  creating.value = true
  formError.value = null
  try {
    const input: CreateCronJobInput = {
      id: form.value.id,
      label: form.value.label.trim(),
      cronExpr: form.value.cronExpr.trim(),
      promptText: form.value.promptText.trim(),
      // Single agent mode; the picked agent is the lead worker (default 通用助手).
      agentMode: 'single',
      leadAgentId: form.value.agentId || DEFAULT_LEAD_AGENT_ID,
      enabled: form.value.enabled
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
  try {
    await updateCronJob(job.id, { enabled })
    await refreshJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
    await refreshJobs()
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
  return `${job.label} · ${cronAgentLabel(job)} · ${describeCron(job.cronExpr)} · ${job.cronExpr} · 下次：${fmtMs(job.nextRunAtMs)} · 上次：${fmtMs(job.lastRunAtMs)}`
}

// Open the cron job's active isolated session in the main panel.
function viewSession(job: CronJob) {
  const sessionId = cronViewSessionId(job)
  if (!sessionId) {
    jobsError.value = '该任务尚未触发，暂无专属会话可查看'
    return
  }
  jobsError.value = null
  chat.openCronConversation(sessionId, job.label, job.leadAgentId, job.agentMode)
  // Close the settings dialog so the user actually sees the conversation they
  // just opened — otherwise the dialog stays on top and the click appears to
  // do nothing.
  emit('view-session')
}

async function submitWebhookSource() {
  const src = webhookSrcInput.value.trim()
  if (!src) { webhookFormError.value = '请填写来源标识'; return }
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(src)) {
    webhookFormError.value = '来源标识以字母或数字开头，仅可含字母、数字、-、_'
    return
  }
  if (!tokenInput.value.trim()) { webhookFormError.value = '请输入 Token'; return }
  const token = tokenInput.value.trim()
  const authHeaderName = authHeaderInput.value.trim() || null
  settingToken.value = true
  webhookFormError.value = null
  webhookError.value = null
  try {
    webhook.value = await setWebhookSourceToken(src, token, authHeaderName)
    rememberWebhookToken(src, token)
    webhookSrcInput.value = ''
    tokenInput.value = ''
    authHeaderInput.value = ''
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
    forgetWebhookToken(src)
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

function webhookViewSessionId(source: WebhookSource): string | null {
  return resolveWebhookViewSessionId(source)
}

function viewWebhookSession(source: WebhookSource) {
  const sessionId = webhookViewSessionId(source)
  if (!sessionId) {
    webhookError.value = '该来源尚未触发，暂无专属会话可查看'
    return
  }
  webhookError.value = null
  chat.openWebhookConversation(sessionId, source.src)
  emit('view-session')
}

function copyWebhookCurl(source: WebhookSource) {
  const token = recallWebhookToken(source.src)
  if (!token) {
    webhookError.value = 'Token 仅在本页创建时保存，请删除后重新添加来源再复制 curl'
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
  loadAgentsList()
  // Webhook 仅 web/server 端可用；桌面端无 HTTP 入口，跳过状态加载。
  if (!isDesktop) refreshWebhook()
})
</script>

<template>
  <div class="space-y-5">
    <!-- Header -->
    <div>
      <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
        <Clock class="w-4 h-4 text-accent" />自动化触发
      </h3>
      <p class="mt-0.5 text-xs text-muted">定时任务（Cron）与 Webhook 调用配置</p>
    </div>

    <!-- ============ Cron jobs ============ -->
    <section class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-3 mb-3">
        <div class="flex items-center gap-2 min-w-0">
          <Clock class="w-4 h-4 text-accent shrink-0" />
          <span class="text-sm font-medium text-foreground whitespace-nowrap">定时任务</span>
          <button
            type="button"
            class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
            :title="CRON_SECTION_DESC"
            aria-label="定时任务说明"
            @click.stop
          >
            <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
          </button>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
            title="刷新"
            :disabled="loadingJobs"
            @click="refreshJobs"
          >
            <RefreshCw class="w-3.5 h-3.5" :class="loadingJobs ? 'animate-spin text-muted' : 'text-muted'" />
          </button>
          <button
            class="h-7 px-3 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 inline-flex items-center gap-1 cursor-pointer whitespace-nowrap shrink-0"
            @click="openCreateForm"
          >
            <Plus class="w-3.5 h-3.5 shrink-0" />新建
          </button>
        </div>
      </div>

      <p v-if="jobsError" class="text-xs text-red-500 mb-2">{{ jobsError }}</p>

      <div v-if="!loadingJobs && jobs.length === 0" class="text-xs text-muted py-4 text-center">
        暂无定时任务
      </div>

      <div v-else class="space-y-2">
        <div
          v-for="job in jobs"
          :key="job.id"
          class="flex items-center gap-2 rounded-lg border border-border bg-card/40 px-3 py-2 min-w-0"
        >
          <button
            class="relative h-5 w-9 rounded-full transition-colors shrink-0 cursor-pointer"
            :class="job.enabled ? 'bg-accent' : 'bg-hover'"
            :title="job.enabled ? '已启用（点击停用）' : '已停用（点击启用）'"
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
            <span class="text-muted truncate">{{ describeCron(job.cronExpr) }}</span>
          </div>
          <div
            class="text-[11px] text-muted shrink-0 whitespace-nowrap"
            :title="`下次：${fmtMs(job.nextRunAtMs)} · 上次：${fmtMs(job.lastRunAtMs)}`"
          >
            下次 {{ fmtMs(job.nextRunAtMs) }}
          </div>
          <div class="flex items-center gap-1 shrink-0">
            <button
              class="h-7 w-7 rounded-md inline-flex items-center justify-center shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
              :class="cronViewSessionId(job)
                ? 'border border-accent/40 bg-accent/10 text-accent hover:bg-accent/15 cursor-pointer'
                : 'border border-border text-muted cursor-not-allowed'"
              :title="cronViewSessionId(job) ? '查看会话' : '任务尚未触发，暂无会话可查看'"
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
              :title="pendingDeleteJobId === job.id ? '确认删除' : '删除'"
              @click="removeJob(job)"
            >
              <Trash2 class="w-3.5 h-3.5" />
            </button>
            <button
              v-if="pendingDeleteJobId === job.id"
              class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center text-muted cursor-pointer shrink-0"
              title="取消"
              @click="cancelDeleteJob"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      <!-- Create form -->
      <div v-if="showForm" class="mt-4 rounded-lg border border-accent/30 bg-accent/5 p-4 space-y-3">
        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="text-[11px] text-muted">名称</span>
            <input v-model="form.label" class="input-base mt-1" placeholder="如：每日日报" />
          </label>
          <label class="block">
            <span class="text-[11px] text-muted">智能体</span>
            <select v-model="form.agentId" class="input-base mt-1">
              <option v-for="a in agentOptions" :key="a.id" :value="a.id">{{ a.label }}</option>
            </select>
          </label>
        </div>
        <label class="block">
          <span class="text-[11px] text-muted">触发提示词</span>
          <textarea v-model="form.promptText" rows="3" class="input-base mt-1 resize-y" placeholder="每次触发时发送给智能体的提示词" />
        </label>
        <CronSchedulePicker v-model="form.cronExpr" />
        <p v-if="formError" class="text-xs text-red-500">{{ formError }}</p>
        <div class="flex items-center justify-end gap-2">
          <button class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer" @click="showForm = false">取消</button>
          <button
            class="h-8 px-4 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
            :disabled="creating"
            @click="submitCreate"
          >
            {{ creating ? '创建中…' : '创建' }}
          </button>
        </div>
      </div>
    </section>

    <!-- ============ Webhook (web/server only) ============ -->
    <section v-if="!isDesktop" class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-3 mb-3">
        <div class="flex items-center gap-2 min-w-0">
          <Webhook class="w-4 h-4 text-accent shrink-0" />
          <span class="text-sm font-medium text-foreground whitespace-nowrap">Webhook 调用</span>
          <button
            type="button"
            class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
            :title="WEBHOOK_SECTION_DESC"
            aria-label="Webhook 说明"
            @click.stop
          >
            <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
          </button>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
            title="刷新"
            :disabled="loadingWebhook"
            @click="refreshWebhook"
          >
            <RefreshCw class="w-3.5 h-3.5" :class="loadingWebhook ? 'animate-spin text-muted' : 'text-muted'" />
          </button>
          <button
            class="h-7 px-3 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 inline-flex items-center gap-1 cursor-pointer whitespace-nowrap shrink-0"
            @click="openWebhookCreateForm"
          >
            <Plus class="w-3.5 h-3.5 shrink-0" />新建
          </button>
        </div>
      </div>

      <p v-if="webhookError" class="text-xs text-red-500 mb-2">{{ webhookError }}</p>

      <div v-if="webhook" class="space-y-3">
        <div
          v-if="webhook.legacyConfigured"
          class="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs"
        >
          <AlertTriangle class="w-3.5 h-3.5 text-amber-500 shrink-0" />
          <span
            class="min-w-0 flex-1 truncate text-foreground"
            :title="`检测到旧版全局 Token（${webhook.legacyPreview}），${LEGACY_TOKEN_DESC}`"
          >
            检测到旧版全局 Token（{{ webhook.legacyPreview }}），{{ LEGACY_TOKEN_DESC }}
          </span>
          <button
            class="h-7 px-2 rounded border border-border hover:bg-hover text-xs cursor-pointer disabled:opacity-50 whitespace-nowrap shrink-0"
            :disabled="clearingLegacy"
            @click="clearLegacyToken"
          >
            {{ clearingLegacy ? '清除中…' : '清除旧 Token' }}
          </button>
        </div>

        <div v-if="!loadingWebhook && webhook.sources.length === 0" class="text-xs text-muted py-4 text-center">
          尚未配置任何来源
        </div>

        <div v-else-if="webhook.sources.length" class="space-y-2">
          <div
            v-for="s in webhook.sources"
            :key="s.src"
            class="flex items-center gap-2 rounded-lg border border-border bg-card/40 px-3 py-2 min-w-0"
          >
            <ShieldCheck class="w-3.5 h-3.5 text-emerald-500 shrink-0" />
            <div
              class="min-w-0 flex-1 flex items-center gap-1.5 text-[12px] truncate"
              :title="`${s.src} · ${s.preview}${s.authHeaderName ? ` · ${s.authHeaderName}` : ''} · ${webhookIngressUrl(s.src)}`"
            >
              <span class="font-medium text-foreground whitespace-nowrap shrink-0">{{ s.src }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="font-mono text-muted truncate">{{ s.preview }}</span>
              <span v-if="s.authHeaderName" class="text-muted/50 shrink-0">·</span>
              <span v-if="s.authHeaderName" class="font-mono text-[10px] text-muted/80 truncate">{{ s.authHeaderName }}</span>
              <span class="text-muted/50 shrink-0">·</span>
              <span class="font-mono text-[10px] text-muted/70 truncate">{{ webhookIngressUrl(s.src) }}</span>
            </div>
            <div class="flex items-center gap-1 shrink-0">
              <button
                class="h-7 w-7 rounded-md inline-flex items-center justify-center shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
                :class="webhookViewSessionId(s)
                  ? 'border border-accent/40 bg-accent/10 text-accent hover:bg-accent/15 cursor-pointer'
                  : 'border border-border text-muted cursor-not-allowed'"
                :title="webhookViewSessionId(s) ? '查看会话' : '尚未触发，暂无会话可查看'"
                :disabled="!webhookViewSessionId(s)"
                @click="viewWebhookSession(s)"
              >
                <MessagesSquare class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center shrink-0 cursor-pointer text-muted"
                :title="recallWebhookToken(s.src) ? '复制 curl' : '复制 curl（需本页创建时保存的 Token）'"
                @click="copyWebhookCurl(s)"
              >
                <Check v-if="copiedWebhookCurlSrc === s.src" class="w-3.5 h-3.5 text-emerald-500" />
                <Copy v-else class="w-3.5 h-3.5" />
              </button>
              <button
                class="h-7 w-7 rounded-md border inline-flex items-center justify-center shrink-0 cursor-pointer disabled:opacity-50"
                :class="pendingDeleteWebhookSrc === s.src
                  ? 'border-danger/40 bg-danger/10 text-danger hover:bg-danger/15'
                  : 'border-border hover:bg-hover text-muted'"
                :title="pendingDeleteWebhookSrc === s.src ? '确认删除' : '删除来源'"
                :disabled="clearingWebhookSrc === s.src"
                @click="removeWebhookSource(s.src)"
              >
                <Trash2 class="w-3.5 h-3.5" />
              </button>
              <button
                v-if="pendingDeleteWebhookSrc === s.src"
                class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center text-muted cursor-pointer shrink-0"
                title="取消"
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
            <div class="flex items-center gap-3 shrink-0">
              <span class="text-[11px] text-muted whitespace-nowrap">来源标识</span>
              <input
                v-model="webhookSrcInput"
                class="input-base w-[20ch] max-w-[20ch] shrink-0"
                placeholder="github"
                :disabled="settingToken"
              />
            </div>
            <label class="block flex-1 min-w-0 space-y-2">
              <span class="text-[11px] text-muted">Token</span>
              <div class="flex items-center gap-2 min-w-0">
                <input
                  v-model="tokenInput"
                  type="password"
                  class="input-base flex-1 min-w-0"
                  placeholder="Bearer Token（仅可设置一次）"
                  :disabled="settingToken"
                />
                <button
                  type="button"
                  class="h-9 w-9 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center shrink-0 cursor-pointer text-muted"
                  title="重新生成 Token"
                  :disabled="settingToken"
                  @click="tokenInput = generateWebhookToken()"
                >
                  <RefreshCw class="w-3.5 h-3.5" />
                </button>
              </div>
            </label>
            <label class="block shrink-0 space-y-2">
              <span class="text-[11px] text-muted whitespace-nowrap">鉴权 Header</span>
              <input
                v-model="authHeaderInput"
                class="input-base w-[24ch] max-w-[24ch]"
                placeholder="X-Codeup-Token"
                :disabled="settingToken"
              />
            </label>
          </div>
          <p class="text-[11px] text-muted">留空则使用 Authorization: Bearer 或 X-Pointer-Token</p>
          <p v-if="webhookFormError" class="text-xs text-red-500">{{ webhookFormError }}</p>
          <div class="flex items-center justify-between gap-3 min-w-0">
            <p class="text-[11px] text-muted min-w-0">自动生成token，添加后可以复制。</p>
            <div class="flex items-center gap-2 shrink-0">
              <button
                class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer"
                @click="showWebhookForm = false"
              >
                取消
              </button>
              <button
                class="h-8 px-4 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
                :disabled="settingToken"
                @click="submitWebhookSource"
              >
                {{ settingToken ? '添加中…' : '添加' }}
              </button>
            </div>
          </div>
        </div>

        <div
          class="rounded-lg border border-border/60 bg-card/30 px-3 py-2.5 space-y-1.5 text-[11px] text-muted leading-relaxed"
        >
          <div class="flex items-start gap-1.5 min-w-0">
            <span class="shrink-0 whitespace-nowrap">地址模板</span>
            <code class="font-mono text-foreground break-all">{{ WEBHOOK_URL_TEMPLATE }}</code>
          </div>
          <div class="min-w-0">
            鉴权：默认 <code class="font-mono text-foreground/90">Authorization: Bearer …</code>
            或 <code class="font-mono text-foreground/90">X-Pointer-Token</code>；
            每个来源可配置自定义 Header（如 Codeup 的 <code class="font-mono text-foreground/90">X-Codeup-Token</code>）
          </div>
          <div class="min-w-0">
            消息：优先 <code class="font-mono text-foreground/90">text</code> /
            <code class="font-mono text-foreground/90">message</code>；无则整段 body 作为消息（兼容 GitHub 等原生 JSON）
          </div>
          <div class="min-w-0" :title="WEBHOOK_REF_BLOCKING">
            同步模式：<code class="font-mono text-foreground/90">"blocking": true</code>，
            可选 <code class="font-mono text-foreground/90">"timeoutSeconds"</code>（默认 120，最大 600）
          </div>
        </div>
      </div>
      <div v-else-if="loadingWebhook" class="text-xs text-muted">加载中…</div>
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
