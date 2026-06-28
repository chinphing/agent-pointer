<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Clock, Plus, Trash2, Webhook, ShieldCheck, AlertTriangle, RefreshCw, MessagesSquare } from 'lucide-vue-next'
import { useChatStore } from '../../../stores/chat'
import {
  listCronJobs,
  createCronJob,
  updateCronJob,
  deleteCronJob,
  listAgents,
  getWebhookConfig,
  setWebhookToken,
  clearWebhookToken
} from '../../../lib/api'
import { isTauriRuntime } from '../../../lib/runtime'
import { composerAgentLabel, resolveAgentUi } from '../../../lib/agentUi'
import { sortComposerAgents } from '../../../lib/agentIcons'
import { describeCron } from '../../../lib/cronSchedule'
import { DEFAULT_LEAD_AGENT_ID } from '../../../types/chat'
import type { AgentDef } from '../../../types/chat'
import type { CronJob, CreateCronJobInput, WebhookConfig } from '../../../types/automation'
import CronSchedulePicker from './CronSchedulePicker.vue'

const emit = defineEmits<{ (e: 'view-session'): void }>()

const chat = useChatStore()

const jobs = ref<CronJob[]>([])
const loadingJobs = ref(false)
const jobsError = ref<string | null>(null)

const webhook = ref<WebhookConfig | null>(null)
const loadingWebhook = ref(false)
const webhookError = ref<string | null>(null)
const tokenInput = ref('')
const settingToken = ref(false)
const clearingToken = ref(false)

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
  if (!confirm(`确定删除定时任务「${job.label}」？`)) return
  try {
    await deleteCronJob(job.id)
    await refreshJobs()
  } catch (e) {
    jobsError.value = (e as Error).message
  }
}

// Open the cron job's active isolated session in the main panel. The active
// session id (`cron:{jobId}:{yyyymmdd}`) is null until the scheduler first
// fires the job; before that there is no transcript to view.
function viewSession(job: CronJob) {
  if (!job.currentSessionId) {
    jobsError.value = '该任务尚未触发，暂无专属会话可查看'
    return
  }
  jobsError.value = null
  chat.openCronConversation(job.currentSessionId, job.label, job.leadAgentId, job.agentMode)
  // Close the settings dialog so the user actually sees the conversation they
  // just opened — otherwise the dialog stays on top and the click appears to
  // do nothing.
  emit('view-session')
}

async function submitToken() {
  if (!tokenInput.value.trim()) { webhookError.value = '请输入 Token'; return }
  settingToken.value = true
  webhookError.value = null
  try {
    webhook.value = await setWebhookToken(tokenInput.value)
    tokenInput.value = ''
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    settingToken.value = false
  }
}

async function resetToken() {
  if (!confirm('确定清除 Webhook Token？清除后可重新设置，但进行中的外部调用将鉴权失败。')) return
  clearingToken.value = true
  webhookError.value = null
  try {
    await clearWebhookToken()
    await refreshWebhook()
  } catch (e) {
    webhookError.value = (e as Error).message
  } finally {
    clearingToken.value = false
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
      <div class="flex items-center justify-between mb-3">
        <div class="flex items-center gap-2">
          <Clock class="w-4 h-4 text-accent" />
          <span class="text-sm font-medium text-foreground">定时任务</span>
          <span class="text-[11px] text-muted">按 Cron 表达式定时触发，每个任务独占一个隔离会话，跨次续接上下文</span>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer"
            title="刷新"
            :disabled="loadingJobs"
            @click="refreshJobs"
          >
            <RefreshCw class="w-3.5 h-3.5" :class="loadingJobs ? 'animate-spin text-muted' : 'text-muted'" />
          </button>
          <button
            class="h-7 px-3 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 inline-flex items-center gap-1 cursor-pointer"
            @click="openCreateForm"
          >
            <Plus class="w-3.5 h-3.5" />新建
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
          class="rounded-lg border border-border bg-card/40 p-3 flex items-center gap-3"
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
          <div class="min-w-0 flex-1">
            <div class="text-[13px] font-medium text-foreground truncate">{{ job.label }}</div>
            <div class="text-[11px] text-muted truncate">{{ describeCron(job.cronExpr) }}</div>
            <div class="text-[10px] text-muted/70 truncate font-mono">{{ job.cronExpr }}</div>
          </div>
          <div class="text-[11px] text-muted text-right shrink-0 hidden sm:block">
            <div>下次：{{ fmtMs(job.nextRunAtMs) }}</div>
            <div>上次：{{ fmtMs(job.lastRunAtMs) }}</div>
          </div>
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0 disabled:opacity-40 disabled:cursor-not-allowed"
            :title="job.currentSessionId ? '查看专属会话' : '任务尚未触发，暂无会话'"
            :disabled="!job.currentSessionId"
            @click="viewSession(job)"
          >
            <MessagesSquare class="w-3.5 h-3.5 text-muted" />
          </button>
          <button
            class="h-7 w-7 rounded-md border border-border hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
            title="删除"
            @click="removeJob(job)"
          >
            <Trash2 class="w-3.5 h-3.5 text-muted" />
          </button>
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
            <span class="text-[11px] text-muted">Agent</span>
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

    <!-- ============ Webhook token (web/server only) ============ -->
    <section v-if="!isDesktop" class="rounded-xl border border-border panel p-5">
      <div class="flex items-center gap-2 mb-3">
        <Webhook class="w-4 h-4 text-accent" />
        <span class="text-sm font-medium text-foreground">Webhook 调用</span>
        <span class="text-[11px] text-muted">外部系统通过 Bearer Token 鉴权触发智能体运行</span>
      </div>

      <p v-if="webhookError" class="text-xs text-red-500 mb-2">{{ webhookError }}</p>

      <!-- Status -->
      <div v-if="webhook" class="space-y-3">
        <div class="flex items-center gap-2 text-[13px]">
          <ShieldCheck v-if="webhook.configured" class="w-4 h-4 text-emerald-500" />
          <AlertTriangle v-else class="w-4 h-4 text-amber-500" />
          <span :class="webhook.configured ? 'text-foreground' : 'text-muted'">
            {{ webhook.configured ? 'Token 已设置' : 'Token 未设置' }}
          </span>
          <span v-if="webhook.configured && webhook.preview" class="font-mono text-xs text-muted">{{ webhook.preview }}</span>
        </div>

        <!-- URL template -->
        <div v-if="webhook.urlTemplate" class="text-xs">
          <span class="text-muted">调用地址模板：</span>
          <code class="font-mono text-foreground break-all">{{ webhook.urlTemplate }}</code>
          <div class="mt-1 text-muted">将 <code>{src}</code> 替换为自定义来源标识，请求体为触发负载。</div>
        </div>

        <!-- First-time set -->
        <div v-if="!webhook.configured" class="flex items-center gap-2">
          <input
            v-model="tokenInput"
            type="password"
            class="input-base flex-1"
            placeholder="输入 Bearer Token（仅可设置一次）"
            :disabled="settingToken"
          />
          <button
            class="h-9 px-4 rounded-md bg-accent text-white text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
            :disabled="settingToken || !tokenInput.trim()"
            @click="submitToken"
          >
            {{ settingToken ? '设置中…' : '设置并加密保存' }}
          </button>
        </div>

        <!-- Configured: locked + reset -->
        <div v-else class="flex items-center gap-2">
          <span class="text-[11px] text-muted">Token 已加密落盘，仅可设置一次。如需更换请先清除。</span>
          <button
            class="h-8 px-3 rounded-md border border-border hover:bg-hover text-xs text-foreground cursor-pointer disabled:opacity-50"
            :disabled="clearingToken"
            @click="resetToken"
          >
            {{ clearingToken ? '清除中…' : '清除 Token' }}
          </button>
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
