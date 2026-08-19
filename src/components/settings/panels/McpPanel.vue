<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Plug, RefreshCw, Plus, Pencil, Trash2, X, ChevronDown, ChevronRight } from 'lucide-vue-next'
import { listMcpServers, saveMcpServers, restartMcpServer } from '../../../lib/api'
import type { GlobalMcpView, GlobalMcpServerView, McpServerDecl } from '../../../types/mcp'

const view = ref<GlobalMcpView | null>(null)
const loading = ref(false)
const saving = ref(false)
const message = ref('')

const STATUS_LABEL: Record<string, string> = {
  healthy: '正常',
  crashed: '已中断',
  degraded: '运行异常',
  stopped: '未启用'
}

/** 弹窗状态：null=关闭；'new'=新增；index=编辑第几项。 */
const modalOpen = ref(false)
const editingIndex = ref<null | 'new' | number>(null)
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
    message.value = `加载失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    loading.value = false
  }
}

async function save() {
  if (!view.value) return
  if (!form.value.name.trim()) {
    message.value = '请填写服务名称'
    return
  }
  saving.value = true
  message.value = ''
  try {
    let decl: GlobalMcpServerView
    if (connType.value === 'http') {
      if (!form.value.url?.trim()) {
        message.value = '请填写服务地址'
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
        headers: Object.keys(headers).length ? headers : null
      }
    } else {
      if (!form.value.command.trim()) {
        message.value = '请填写启动命令'
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
        env: parseEnv(envText.value)
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
    message.value = '配置已保存'
  } catch (e) {
    message.value = `保存失败: ${e instanceof Error ? e.message : String(e)}`
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
    message.value = '已删除'
  } catch (e) {
    message.value = `删除失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    saving.value = false
  }
}

async function restart() {
  saving.value = true
  message.value = ''
  try {
    view.value = await restartMcpServer()
    message.value = '服务已重启'
  } catch (e) {
    message.value = `重启失败: ${e instanceof Error ? e.message : String(e)}`
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
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-base font-semibold text-gray-800 dark:text-gray-100">外部工具服务（MCP）</h2>
        <p class="text-xs text-gray-500 dark:text-gray-400">
          连接外部服务提供的工具，在对话中即可直接调用，无需安装插件。
          配置保存在本机，添加后立即生效。
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-ghost btn-sm" :disabled="saving || loading" title="重新获取服务状态" @click="refresh">
          <RefreshCw class="w-4 h-4" />
          刷新状态
        </button>
        <button class="btn btn-ghost btn-sm" :disabled="saving || loading" title="重新启动所有服务" @click="restart">
          <Plug class="w-4 h-4" />
          重启服务
        </button>
        <button class="btn btn-primary btn-sm" :disabled="saving || loading" @click="openNew">
          <Plus class="w-4 h-4" />
          添加服务
        </button>
      </div>
    </div>

    <p v-if="message" class="text-xs" :class="message.includes('失败') || message.startsWith('加载') ? 'text-red-500' : 'text-green-600'">
      {{ message }}
    </p>

    <div v-if="!view" class="text-sm text-gray-500 dark:text-gray-400 py-8 text-center">
      {{ loading ? '加载中…' : '暂无数据' }}
    </div>

    <div v-else-if="view.servers.length === 0" class="py-12 text-center flex flex-col items-center gap-3">
      <div class="w-12 h-12 rounded-full bg-blue-50 dark:bg-blue-950/50 flex items-center justify-center">
        <Plug class="w-6 h-6 text-blue-400" />
      </div>
      <div>
        <div class="text-sm font-medium text-gray-700 dark:text-gray-200">还没有外部工具服务</div>
        <div class="text-xs text-gray-400 mt-1">添加一个服务，对话中即可直接调用它的工具</div>
      </div>
      <button class="btn btn-primary btn-sm" :disabled="saving || loading" @click="openNew">
        <Plus class="w-4 h-4" />
        添加服务
      </button>
    </div>

    <div v-else class="space-y-2">
      <div
        v-for="(s, index) in view.servers"
        :key="s.name"
        class="flex items-center justify-between rounded-xl border border-border bg-card px-4 py-3"
      >
        <div class="flex items-center gap-3 min-w-0">
          <div class="w-8 h-8 rounded-lg bg-blue-50 dark:bg-blue-950/50 flex items-center justify-center shrink-0">
            <Plug class="w-4 h-4 text-blue-500" />
          </div>
          <div class="min-w-0">
            <div class="text-sm font-medium text-gray-800 dark:text-gray-100 flex items-center gap-2">
              {{ s.name }}
              <span
                class="text-[10px] px-1.5 py-0.5 rounded-full"
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
            <div class="text-xs text-gray-500 dark:text-gray-400 font-mono truncate">{{ s.command }}</div>
            <div v-if="s.lastError" class="text-xs text-red-500 truncate" :title="s.lastError">{{ s.lastError }}</div>
          </div>
        </div>
        <div class="flex items-center gap-1 shrink-0">
          <span v-if="s.restartCount > 0" class="text-xs text-gray-400 mr-1">自动重启 {{ s.restartCount }} 次</span>
          <button class="p-1.5 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-hover dark:hover:text-gray-200" :disabled="saving" title="编辑" @click="openEdit(index)">
            <Pencil class="w-4 h-4" />
          </button>
          <button class="p-1.5 rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-950/30" :disabled="saving" title="删除" @click="remove(index)">
            <Trash2 class="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>

    <!-- 添加 / 编辑弹窗 -->
    <div v-if="modalOpen" class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" @click.self="closeModal">
      <div class="w-full max-w-md rounded-2xl bg-background border border-border shadow-xl p-5 flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-semibold text-gray-800 dark:text-gray-100">
            {{ editingIndex === 'new' ? '添加服务' : '编辑服务' }}
          </h3>
          <button class="p-1.5 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-hover" title="关闭" @click="closeModal">
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="flex flex-col gap-3">
          <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
            <span>服务名称 <span class="text-red-500">*</span></span>
            <input v-model="form.name" class="input input-sm" placeholder="例如：天气服务、数据库助手" />
          </label>

          <!-- 连接方式 -->
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="flex-1 rounded-lg border px-3 py-2 text-xs text-left transition-colors"
              :class="connType === 'http' ? 'border-blue-500 bg-blue-50 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300' : 'border-border text-gray-600 dark:text-gray-300 hover:bg-hover'"
              @click="connType = 'http'"
            >
              <div class="font-medium">连接远程服务</div>
              <div class="text-[11px] opacity-70">填服务地址（URL），适合使用别人提供的服务</div>
            </button>
            <button
              type="button"
              class="flex-1 rounded-lg border px-3 py-2 text-xs text-left transition-colors"
              :class="connType === 'stdio' ? 'border-blue-500 bg-blue-50 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300' : 'border-border text-gray-600 dark:text-gray-300 hover:bg-hover'"
              @click="connType = 'stdio'"
            >
              <div class="font-medium">启动本机程序</div>
              <div class="text-[11px] opacity-70">填程序或脚本路径，适合自己开发的服务</div>
            </button>
          </div>

          <template v-if="connType === 'http'">
            <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
              <span>服务地址 <span class="text-red-500">*</span></span>
              <input v-model="form.url" class="input input-sm font-mono" placeholder="https://example.com/mcp" />
            </label>
            <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
              <span>访问令牌（可选）</span>
              <input v-model="tokenText" class="input input-sm font-mono" placeholder="服务方提供的 API 令牌" />
            </label>
          </template>
          <template v-else>
            <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
              <span>启动命令 <span class="text-red-500">*</span></span>
              <input v-model="form.command" class="input input-sm font-mono" placeholder="程序路径或脚本，例如 /usr/local/bin/my-mcp" />
            </label>

            <button class="flex items-center gap-1 text-xs text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 self-start" @click="showAdvanced = !showAdvanced">
              <ChevronDown v-if="showAdvanced" class="w-3.5 h-3.5" />
              <ChevronRight v-else class="w-3.5 h-3.5" />
              高级设置
            </button>

            <div v-if="showAdvanced" class="flex flex-col gap-3 pl-3 border-l-2 border-border">
              <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
                <span>命令参数（可选，空格分隔）</span>
                <input v-model="argsText" class="input input-sm font-mono" placeholder="例如：serve --port 8080" />
              </label>
              <label class="flex flex-col gap-1.5 text-xs text-gray-600 dark:text-gray-300">
                <span>环境变量（可选，每行一个 KEY=VALUE）</span>
                <textarea v-model="envText" rows="3" class="textarea textarea-sm font-mono resize-none" placeholder="TOKEN=replace-me" />
              </label>
            </div>
          </template>
        </div>

        <div class="flex items-center justify-end gap-2 pt-1">
          <button class="btn btn-ghost btn-sm" :disabled="saving" @click="closeModal">取消</button>
          <button class="btn btn-primary btn-sm" :disabled="saving" @click="save">
            {{ saving ? '保存中…' : '保存' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
