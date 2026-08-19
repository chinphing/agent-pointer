<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Plug, RefreshCw, RotateCw, Server } from 'lucide-vue-next'
import { listMcpServers, reloadMcpServers, restartMcpServer } from '../../../lib/api'
import type { GlobalMcpView } from '../../../types/mcp'

const view = ref<GlobalMcpView | null>(null)
const loading = ref(false)
const busy = ref(false)
const message = ref('')

const STATUS_LABEL: Record<string, string> = {
  healthy: '正常',
  crashed: '已中断（等待自动恢复）',
  degraded: '运行异常（已停止自动恢复）',
  stopped: '未启用'
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

async function reload() {
  busy.value = true
  message.value = ''
  try {
    view.value = await reloadMcpServers()
    message.value = '配置已应用'
  } catch (e) {
    message.value = `应用更改失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    busy.value = false
  }
}

async function restart() {
  busy.value = true
  message.value = ''
  try {
    view.value = await restartMcpServer()
    message.value = '服务已重启'
  } catch (e) {
    message.value = `重启失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    busy.value = false
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
          连接外部服务提供的工具，启用后即可在对话中直接调用，无需安装插件。
          通过配置文件添加服务，修改后点击「应用更改」生效。
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="refresh">
          <RefreshCw class="w-4 h-4" />
          刷新状态
        </button>
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="reload">
          <RotateCw class="w-4 h-4" />
          应用更改
        </button>
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="restart">
          <Server class="w-4 h-4" />
          重启服务
        </button>
      </div>
    </div>

    <p v-if="message" class="text-xs" :class="message.startsWith('失败') || message.startsWith('加载') || message.startsWith('应用') || message.startsWith('重启') ? 'text-red-500' : 'text-green-600'">
      {{ message }}
    </p>

    <div v-if="!view" class="text-sm text-gray-500 dark:text-gray-400 py-8 text-center">
      {{ loading ? '加载中…' : '暂无数据' }}
    </div>

    <div v-else-if="view.servers.length === 0" class="text-sm text-gray-500 dark:text-gray-400 py-8 text-center flex flex-col items-center gap-2">
      <Plug class="w-8 h-8 opacity-40" />
      <span>尚未添加任何外部工具服务</span>
      <span class="text-xs">
        在配置文件 <code class="font-mono">pointer-server.toml</code> 中添加
        <code class="font-mono">[[mcp_servers.server]]</code> 后，点击「应用更改」即可启用。
      </span>
    </div>

    <div v-else class="space-y-2">
      <div
        v-for="s in view.servers"
        :key="s.name"
        class="flex items-center justify-between rounded-lg border border-gray-200 dark:border-gray-700 px-4 py-3"
      >
        <div class="flex items-center gap-3 min-w-0">
          <Plug class="w-4 h-4 text-blue-500 shrink-0" />
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
        <div v-if="s.restartCount > 0" class="text-xs text-gray-400 shrink-0">已自动重启 {{ s.restartCount }} 次</div>
      </div>
    </div>
  </div>
</template>
