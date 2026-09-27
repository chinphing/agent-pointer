<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()
import { computed, onMounted, ref } from 'vue'
import { Plug, RefreshCw, Plus, Pencil, Trash2, X, ChevronDown, ChevronRight, Eye } from 'lucide-vue-next'
import { listMcpServers, saveMcpServers, restartMcpServer } from '../../../lib/api'
import type { GlobalMcpView, GlobalMcpServerView, McpServerDecl } from '../../../types/mcp'

const view = ref<GlobalMcpView | null>(null)
const loading = ref(false)
const saving = ref(false)
const message = ref('')

const STATUS_LABEL = computed((): Record<string, string> => ({
  healthy: t('settings.mcp.statusHealthy'),
  crashed: t('settings.mcp.statusCrashed'),
  degraded: t('settings.mcp.statusDegraded'),
  stopped: t('settings.mcp.statusStopped')
}))

const healthyCount = computed(
  () => view.value?.servers.filter(s => s.status === 'healthy').length ?? 0
)

/** 成功/失败消息配色：失败（含加载失败）用 danger，其余用常规前景色。 */
const isError = computed(
  () => message.value.includes(t('common.error')) || message.value.includes('fail') || message.value.includes(t('chat.toolFailed')) || message.value.startsWith(t('settings.loadFailed').slice(0,2)) || message.value.startsWith(t('settings.s_4793ea')) || message.value.toLowerCase().includes('fail')
)

/** 列表主行展示的连接目标：http 显示地址，stdio 显示命令。 */
function displayTarget(s: GlobalMcpServerView): string {
  return s.transport === 'http' ? (s.url ?? s.command) : s.command
}

/** 弹窗状态：null=关闭；'new'=新增；index=编辑第几项。 */
const modalOpen = ref(false)
const editingIndex = ref<null | 'new' | number>(null)
/** 工具列表弹窗：正在查看哪个服务。 */
const toolsModal = ref<GlobalMcpServerView | null>(null)
/** 连接方式：http=远程服务；stdio=本机命令 */
const connType = ref<'http' | 'stdio'>('http')
const form = ref<McpServerDecl>({ name: '', transport: 'http', command: '', args: [], env: {}, url: '', headers: null })
const argsText = ref('')
const envText = ref('')
const tokenText = ref('')
const showAdvanced = ref(false)

function openNew() {
  editingIndex.value = 'new'
  connType.value = 'http'
  form.value = { name: '', transport: 'http', command: '', args: [], env: {}, url: '', headers: null }
  argsText.value = ''
  envText.value = ''
  tokenText.value = ''
  showAdvanced.value = false
  modalOpen.value = true
}

function openEdit(index: number) {
  const s = view.value?.servers[index]
  if (!s) return
  editingIndex.value = index
  const isHttp = s.transport === 'http'
  connType.value = isHttp ? 'http' : 'stdio'
  form.value = {
    name: s.name,
    transport: s.transport,
    command: s.command,
    args: [...s.args],
    env: { ...s.env },
    url: s.url ?? '',
    headers: s.headers ? { ...s.headers } : null
  }
  argsText.value = s.args.join(' ')
  envText.value = Object.entries(s.env)
    .map(([k, v]) => `${k}=${v}`)
    .join('\n')
  tokenText.value = isHttp ? (s.headers?.Authorization ?? '').replace(/^Bearer\s+/i, '') : ''
  showAdvanced.value = s.args.length > 0 || Object.keys(s.env).length > 0
  modalOpen.value = true
}

function closeModal() {
  modalOpen.value = false
}

function parseArgs(text: string): string[] {
  return text
    .split(/\s+/)
    .map(s => s.trim())
    .filter(Boolean)
}

function parseEnv(text: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    const eq = trimmed.indexOf('=')
    if (eq > 0) {
      env[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim()
    }
  }
  return env
}

async function refresh() {
  loading.value = true
  message.value = ''
  try {
    view.value = await listMcpServers()
  } catch (e) {
    message.value = t('settings.mcp.loadFailed', { error: e instanceof Error ? e.message : String(e) })
  } finally {
    loading.value = false
  }
}

async function save() {
  if (!view.value) return
  if (!form.value.name.trim()) {
    message.value = t('settings.mcp.nameRequired')
    return
  }
  saving.value = true
  message.value = ''
  try {
    let decl: GlobalMcpServerView
    if (connType.value === 'http') {
      if (!form.value.url?.trim()) {
        message.value = t('settings.mcp.urlRequired')
        saving.value = false
        return
      }
      const headers: Record<string, string> = {}
      if (tokenText.value.trim()) {
        headers.Authorization = `Bearer ${tokenText.value.trim()}`
      }
      decl = {
        name: form.value.name.trim(),
        transport: 'http',
        command: '',
        status: 'stopped',
        restartCount: 0,
        lastError: null,
        args: [],
        env: {},
        url: form.value.url.trim(),
        headers: Object.keys(headers).length ? headers : null,
        tools: []
      }
    } else {
      if (!form.value.command.trim()) {
        message.value = t('settings.mcp.cmdRequired')
        saving.value = false
        return
      }
      decl = {
        name: form.value.name.trim(),
        transport: 'stdio',
        command: form.value.command.trim(),
        status: 'stopped',
        restartCount: 0,
        lastError: null,
        args: parseArgs(argsText.value),
        env: parseEnv(envText.value),
        tools: []
      }
    }
    const servers = [...view.value.servers]
    if (editingIndex.value === 'new') {
      servers.push(decl)
    } else if (typeof editingIndex.value === 'number') {
      servers[editingIndex.value] = decl
    }
    view.value = await saveMcpServers(servers)
    modalOpen.value = false
    message.value = t('settings.mcp.saved')
  } catch (e) {
    message.value = t('settings.mcp.saveFailed', { error: e instanceof Error ? e.message : String(e) })
  } finally {
    saving.value = false
  }
}

async function remove(index: number) {
  if (!view.value) return
  saving.value = true
  message.value = ''
  try {
    const servers = [...view.value.servers]
    servers.splice(index, 1)
    view.value = await saveMcpServers(servers)
    message.value = t('settings.mcp.deleted')
  } catch (e) {
    message.value = t('settings.mcp.deleteFailed', { error: e instanceof Error ? e.message : String(e) })
  } finally {
    saving.value = false
  }
}

async function restart() {
  saving.value = true
  message.value = ''
  try {
    view.value = await restartMcpServer()
    message.value = t('settings.mcp.restarted')
  } catch (e) {
    message.value = t('settings.mcp.restartFailed', { error: e instanceof Error ? e.message : String(e) })
  } finally {
    saving.value = false
  }
}

onMounted(() => {
  void refresh()
})
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex items-start justify-end gap-4">
      <div class="flex items-center gap-2 shrink-0">
        <button
          type="button"
          class="h-9 inline-flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="saving || loading"
          :title="t('settings.mcp.refreshTitle')"
          @click="refresh"
        >
          <RefreshCw class="w-4 h-4" aria-hidden="true" />
          {{ t('settings.mcp.refresh') }}
        </button>
        <button
          type="button"
          class="h-9 inline-flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="saving || loading"
          :title="t('settings.mcp.restartAllTitle')"
          @click="restart"
        >
          <Plug class="w-4 h-4" aria-hidden="true" />
          {{ t('settings.mcp.restart') }}
        </button>
        <button
          type="button"
          class="h-9 inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 text-sm font-medium text-accent-foreground hover:opacity-90 cursor-pointer transition-opacity disabled:opacity-50"
          :disabled="saving || loading"
          @click="openNew"
        >
          <Plus class="w-4 h-4" aria-hidden="true" />
          {{ t('settings.mcp.add') }}
        </button>
      </div>
    </div>

    <div
      v-if="message"
      class="rounded-xl border border-border bg-accent-muted/50 px-3 py-2 text-sm whitespace-pre-line"
      :class="isError ? 'text-danger' : 'text-foreground'"
    >
      {{ message }}
    </div>

    <div v-if="!view" class="text-sm text-muted py-8 text-center">
      {{ loading ? t('settings.mcp.loading') : t('settings.mcp.noData') }}
    </div>

    <div
      v-else-if="view.servers.length === 0"
      class="rounded-2xl border border-dashed border-border py-10 text-center flex flex-col items-center gap-3"
    >
      <Plug class="w-6 h-6 text-muted" aria-hidden="true" />
      <div class="text-sm text-muted">{{ t('settings.mcp.empty') }}</div>
      <button
        type="button"
        class="h-8 inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer transition-opacity disabled:opacity-50"
        :disabled="saving || loading"
        @click="openNew"
      >
        <Plus class="w-4 h-4" aria-hidden="true" />
        {{ t('settings.mcp.add') }}
      </button>
    </div>

    <div v-else class="space-y-3">
      <div class="flex items-center justify-between text-xs text-muted">
        <span>{{ t('settings.mcp.summary', { total: view.servers.length, healthy: healthyCount }) }}</span>
      </div>
      <div
        v-for="(s, index) in view.servers"
        :key="s.name"
        class="flex items-center justify-between gap-3 rounded-xl border border-border bg-card px-3 py-2"
      >
        <div class="flex items-center gap-3 min-w-0">
          <div class="w-8 h-8 rounded-lg bg-hover flex items-center justify-center shrink-0">
            <Plug class="w-4 h-4 text-muted" aria-hidden="true" />
          </div>
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h4 class="truncate text-sm font-semibold text-foreground">{{ s.name }}</h4>
              <span
                class="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium"
                :class="{
                  'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300': s.status === 'healthy',
                  'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300': s.status === 'crashed',
                  'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300': s.status === 'degraded',
                  'bg-gray-100 text-gray-500 dark:bg-gray-800 dark:text-gray-400': s.status === 'stopped'
                }"
              >
                {{ STATUS_LABEL[s.status] ?? s.status }}
              </span>
            </div>
            <p class="truncate text-xs text-muted font-mono">{{ displayTarget(s) }}</p>
            <p v-if="s.lastError" class="truncate text-xs text-danger" :title="s.lastError">{{ s.lastError }}</p>
          </div>
        </div>
        <div class="flex items-center gap-1 shrink-0">
          <span v-if="s.restartCount > 0" class="text-xs text-muted mr-1">{{ t('settings.mcp.autoRestart', { count: s.restartCount }) }}</span>
          <button
            type="button"
            class="p-1.5 rounded-lg text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors"
            :disabled="saving"
            :title="t('settings.mcp.viewTools')"
            @click="toolsModal = s"
          >
            <Eye class="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="p-1.5 rounded-lg text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors"
            :disabled="saving"
            :title="t('settings.mcp.editTitle')"
            @click="openEdit(index)"
          >
            <Pencil class="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="p-1.5 rounded-lg text-muted hover:text-danger hover:bg-danger/10 cursor-pointer transition-colors"
            :disabled="saving"
            :title="t('settings.mcp.deleteTitle')"
            @click="remove(index)"
          >
            <Trash2 class="w-4 h-4" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>

    <!-- 添加 / 编辑弹窗 -->
    <Teleport to="body">
      <div
        v-if="modalOpen"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="closeModal"
      >
        <div class="w-full max-w-md max-h-[85vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">
                {{ editingIndex === 'new' ? t('settings.mcp.add') : t('settings.mcp.edit') }}
              </h4>
              <p class="mt-0.5 text-[11px] text-muted">
                {{ connType === 'http' ? t('settings.mcp.httpHint') : t('settings.mcp.stdioHint') }}
              </p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :title="t('common.close')"
              @click="closeModal"
            >
              <X class="w-4 h-4" aria-hidden="true" />
            </button>
          </div>

          <div class="px-5 py-4 overflow-y-auto flex flex-col gap-4">
            <label class="block">
              <span class="text-[11px] text-muted">{{ t('settings.mcp.name') }} <span class="text-danger">*</span></span>
              <input v-model="form.name" class="input-base mt-1" :placeholder="t('settings.mcp.namePlaceholder')" />
            </label>

            <!-- 连接方式 -->
            <div class="grid grid-cols-2 gap-2">
              <button
                type="button"
                class="rounded-lg border px-3 py-2 text-left transition-colors cursor-pointer min-h-[3.75rem] flex flex-col justify-center"
                :class="connType === 'http' ? 'border-accent/60 bg-accent/10' : 'border-border bg-card hover:bg-hover'"
                @click="connType = 'http'"
              >
                <span class="text-xs font-medium" :class="connType === 'http' ? 'text-accent' : 'text-foreground'">{{ t('settings.mcp.remote') }}</span>
                <span class="mt-0.5 text-[11px] text-muted">{{ t('settings.mcp.remoteHint') }}</span>
              </button>
              <button
                type="button"
                class="rounded-lg border px-3 py-2 text-left transition-colors cursor-pointer min-h-[3.75rem] flex flex-col justify-center"
                :class="connType === 'stdio' ? 'border-accent/60 bg-accent/10' : 'border-border bg-card hover:bg-hover'"
                @click="connType = 'stdio'"
              >
                <span class="text-xs font-medium" :class="connType === 'stdio' ? 'text-accent' : 'text-foreground'">{{ t('settings.mcp.local') }}</span>
                <span class="mt-0.5 text-[11px] text-muted">{{ t('settings.mcp.localHint') }}</span>
              </button>
            </div>

            <template v-if="connType === 'http'">
              <label class="block">
                <span class="text-[11px] text-muted">{{ t('settings.mcp.url') }} <span class="text-danger">*</span></span>
                <input v-model="form.url" class="input-base mt-1 font-mono" placeholder="https://example.com/mcp" />
              </label>
              <label class="block">
                <span class="text-[11px] text-muted">{{ t('settings.mcp.tokenOptional') }}</span>
                <input v-model="tokenText" class="input-base mt-1 font-mono" :placeholder="t('settings.mcp.tokenPlaceholder')" />
              </label>
            </template>
            <template v-else>
              <label class="block">
                <span class="text-[11px] text-muted">{{ t('settings.mcp.command') }} <span class="text-danger">*</span></span>
                <input v-model="form.command" class="input-base mt-1 font-mono" :placeholder="t('settings.usrLocalBinMy_e908cb')" />
              </label>

              <button
                type="button"
                class="flex items-center gap-1 text-xs text-muted hover:text-foreground cursor-pointer self-start transition-colors"
                @click="showAdvanced = !showAdvanced"
              >
                <ChevronDown v-if="showAdvanced" class="w-3.5 h-3.5" aria-hidden="true" />
                <ChevronRight v-else class="w-3.5 h-3.5" aria-hidden="true" />
                {{ t('settings.mcp.advanced') }}
              </button>

              <div v-if="showAdvanced" class="flex flex-col gap-3 pl-3 border-l-2 border-border">
                <label class="block">
                  <span class="text-[11px] text-muted">{{ t('settings.mcp.argsOptional') }}</span>
                  <input v-model="argsText" class="input-base mt-1 font-mono" :placeholder="t('settings.servePort8080_d922fd')" />
                </label>
                <label class="block">
                  <span class="text-[11px] text-muted">{{ t('settings.mcp.envOptional') }}</span>
                  <textarea v-model="envText" rows="3" class="input-base mt-1 font-mono resize-none" placeholder="TOKEN=replace-me" />
                </label>
              </div>
            </template>
          </div>

          <div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3 shrink-0">
            <button type="button" class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer" :disabled="saving" @click="closeModal">{{ t('common.cancel') }}</button>
            <button
              type="button"
              class="h-8 px-4 rounded-md bg-accent text-accent-foreground text-xs font-medium hover:opacity-95 cursor-pointer disabled:opacity-50"
              :disabled="saving"
              @click="save"
            >
              {{ saving ? t('settings.mcp.saving') : t('common.save') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 工具列表弹窗 -->
    <Teleport to="body">
      <div
        v-if="toolsModal"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="toolsModal = null"
      >
        <div class="w-full max-w-lg max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.mcp.toolsTitle', { name: toolsModal.name }) }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.mcp.toolsHint') }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :title="t('common.close')"
              @click="toolsModal = null"
            >
              <X class="w-4 h-4" aria-hidden="true" />
            </button>
          </div>

          <div class="px-5 py-4 overflow-y-auto flex flex-col gap-2">
            <div
              v-if="toolsModal.tools.length === 0"
              class="text-sm text-muted py-6 text-center"
            >
              {{ toolsModal.status === 'healthy' ? t('settings.mcp.noToolsHealthy') : t('settings.mcp.noToolsOffline') }}
            </div>
            <div
              v-for="t in toolsModal.tools"
              :key="t.name"
              class="rounded-lg border border-border bg-hover/50 px-3 py-2"
            >
              <div class="text-xs font-mono font-medium text-foreground break-all">{{ t.name }}</div>
              <p v-if="t.description" class="mt-1 text-[11px] text-muted leading-relaxed whitespace-pre-line">{{ t.description }}</p>
            </div>
          </div>

          <div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3 shrink-0">
            <span class="text-[11px] text-muted mr-auto">{{ t('settings.mcp.toolsCount', { count: toolsModal.tools.length }) }}</span>
            <button
              type="button"
              class="h-8 px-3 rounded-md bg-hover hover:bg-hover text-xs text-foreground cursor-pointer"
              @click="toolsModal = null"
            >
              {{ t('common.close') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
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
</style>
