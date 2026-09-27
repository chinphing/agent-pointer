<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Loader2, RefreshCw } from 'lucide-vue-next'
import { listTokenUsage, type TokenUsageListResult } from '../../../lib/api'
import { agentRoleLabel } from '../../../lib/agentLabels'

type RangeKey = 'today' | 'yesterday' | 'week' | 'month'

const rangeOptions: { key: RangeKey; label: string }[] = [
  { key: 'today', label: '今天' },
  { key: 'yesterday', label: '昨天' },
  { key: 'week', label: '最近一周' },
  { key: 'month', label: '最近一月' }
]

const activeRange = ref<RangeKey>('month')
const loading = ref(false)
const error = ref<string | null>(null)
const result = ref<TokenUsageListResult | null>(null)

function rangeBounds(key: RangeKey): { from: string; to: string } {
  const now = new Date()
  const end = new Date(now)
  end.setHours(0, 0, 0, 0)
  end.setDate(end.getDate() + 1)
  const start = new Date(end)
  if (key === 'today') {
    start.setDate(start.getDate() - 1)
  } else if (key === 'yesterday') {
    start.setDate(start.getDate() - 2)
    end.setDate(end.getDate() - 1)
  } else if (key === 'week') {
    start.setDate(start.getDate() - 7)
  } else {
    start.setMonth(start.getMonth() - 1)
  }
  return { from: start.toISOString(), to: end.toISOString() }
}

async function load() {
  loading.value = true
  error.value = null
  try {
    const { from, to } = rangeBounds(activeRange.value)
    result.value = await listTokenUsage(from, to)
  } catch (e) {
    console.error('[usage-panel] list token usage failed', e)
    error.value = e instanceof Error ? e.message : String(e)
    result.value = null
  } finally {
    loading.value = false
  }
}

function pickRange(key: RangeKey) {
  if (activeRange.value === key) return
  activeRange.value = key
  void load()
}

const items = computed(() => result.value?.items ?? [])

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const pad = (v: number) => String(v).padStart(2, '0')
  return `${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

onMounted(() => {
  void load()
})
</script>

<template>
  <div class="flex flex-col gap-4 p-4">
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-1.5">
        <button
          v-for="opt in rangeOptions"
          :key="opt.key"
          type="button"
          class="rounded-lg px-2.5 py-1 text-[12px] transition-colors cursor-pointer"
          :class="activeRange === opt.key
            ? 'bg-hover text-foreground font-medium'
            : 'text-muted hover:bg-hover hover:text-foreground'"
          @click="pickRange(opt.key)"
        >
          {{ opt.label }}
        </button>
      </div>
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-foreground transition-colors cursor-pointer disabled:opacity-50"
        :disabled="loading"
        title="刷新"
        aria-label="刷新"
        @click="load"
      >
        <RefreshCw class="h-3.5 w-3.5" :class="loading ? 'animate-spin' : ''" />
      </button>
    </div>

    <div v-if="result" class="flex items-baseline gap-4 text-[12px] text-muted">
      <span>合计 <span class="text-foreground font-semibold">{{ formatTokens(result.totalTokens) }}</span> tokens</span>
      <span>输入 {{ formatTokens(result.totalPromptTokens) }}</span>
      <span>输出 {{ formatTokens(result.totalCompletionTokens) }}</span>
    </div>

    <p v-if="error" class="text-[12px] text-danger">{{ error }}</p>

    <div v-if="loading && !result" class="flex items-center justify-center py-10 text-muted">
      <Loader2 class="h-4 w-4 animate-spin" />
    </div>

    <p v-else-if="!items.length" class="py-10 text-center text-[12px] text-muted">
      该时间范围内没有消耗记录
    </p>

    <div v-else class="overflow-auto rounded-lg border border-border">
      <table class="w-full text-[12px]">
        <thead>
          <tr class="border-b border-border bg-hover/40 text-left text-muted">
            <th class="px-2.5 py-1.5 font-medium">时间</th>
            <th class="px-2.5 py-1.5 font-medium">模型</th>
            <th class="px-2.5 py-1.5 font-medium">智能体</th>
            <th class="px-2.5 py-1.5 font-medium text-right">输入</th>
            <th class="px-2.5 py-1.5 font-medium text-right">输出</th>
            <th class="px-2.5 py-1.5 font-medium text-right">合计</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in items"
            :key="`${row.runId}:${row.agentInstanceId}:${row.modelName}`"
            class="border-b border-border/50 last:border-0"
          >
            <td class="px-2.5 py-1.5 text-muted whitespace-nowrap">{{ formatTime(row.updatedAt) }}</td>
            <td class="px-2.5 py-1.5 text-foreground/90">{{ row.modelName }}</td>
            <td class="px-2.5 py-1.5 text-muted">{{ agentRoleLabel(row.agentRoleId) || '—' }}</td>
            <td class="px-2.5 py-1.5 text-right tabular-nums text-foreground/80">{{ formatTokens(row.promptTokens) }}</td>
            <td class="px-2.5 py-1.5 text-right tabular-nums text-foreground/80">{{ formatTokens(row.completionTokens) }}</td>
            <td class="px-2.5 py-1.5 text-right tabular-nums font-medium text-foreground">{{ formatTokens(row.totalTokens) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
