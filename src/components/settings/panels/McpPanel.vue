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
  crashed: '已崩溃（等待重启）',
  degraded: '运行异常（重试达上限）',
  stopped: '未启动'
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
    message.value = '已重新读取配置并应用'
  } catch (e) {
    message.value = `重载失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    busy.value = false
  }
}

async function restart() {
  busy.value = true
  message.value = ''
  try {
    view.value = await restartMcpServer()
    message.value = '已重启全部全局 MCP server'
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
        <h2 class="text-base font-semibold text-gray-800 dark:text-gray-100">全局 MCP server</h2>
        <p class="text-xs text-gray-500 dark:text-gray-400">
          在 <code class="font-mono">pointer-server.toml</code> 配置
          <code class="font-mono">[[mcp_servers.server]]</code>，不依赖插件即可使用；改配置后点「重载配置」生效。
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="refresh">
          <RefreshCw class="w-4 h-4" />
          刷新
        </button>
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="reload">
          <RotateCw class="w-4 h-4" />
          重载配置
        </button>
        <button class="btn btn-ghost btn-sm" :disabled="busy || loading" @click="restart">
          <Server class="w-4 h-4" />
          重启
        </button>
      </div>
    </div>

    <p v-if="message" class="text-xs" :class="message.startsWith('失败') || message.startsWith('加载') || message.startsWith('重载') || message.startsWith('重启') ? 'text-red-500' : 'text-green-600'">
      {{ message }}
    </p>

    <div v-if="!view" class="text-sm text-gray-500 dark:text-gray-400 py-8 text-center">
      {{ loading ? '加载中…' : '暂无数据' }}
    </div>

    <div v-else-if="view.servers.length === 0" class="text-sm text-gray-500 dark:text-gray-400 py-8 text-center flex flex-col items-center gap-2">
      <Plug class="w-8 h-8 opacity-40" />
      <span>未配置全局 MCP server</span>
      <span class="text-xs">在 pointer-server.toml 添加 [[mcp_servers.server]] 后点击「重载配置」</span>
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
        <div v-if="s.restartCount > 0" class="text-xs text-gray-400 shrink-0">重启 {{ s.restartCount }} 次</div>
      </div>
    </div>
  </div>
</template>
