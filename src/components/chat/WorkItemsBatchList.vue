<script setup lang="ts">
import { computed, toRef, watch } from 'vue'
import { CheckCircle2, Circle, Loader2, XCircle, Ban, ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { useWorkItemsList } from '../../composables/useWorkItemsList'

const props = defineProps<{
  conversationId: string
  batchId: string
  batchTitle: string
  taskId?: string
  enabled: boolean
  refreshKey: string
  open: boolean
}>()

const emit = defineEmits<{
  'update:open': [value: boolean]
}>()

const {
  items,
  total,
  offset,
  stats,
  loading,
  error,
  setExpanded,
  nextPage,
  prevPage,
  pageSize
} = useWorkItemsList({
  conversationId: toRef(props, 'conversationId'),
  taskId: toRef(props, 'taskId'),
  batchId: toRef(props, 'batchId'),
  enabled: toRef(props, 'enabled'),
  refreshKey: toRef(props, 'refreshKey')
})

watch(
  () => props.open,
  open => setExpanded(open),
  { immediate: true }
)

const statsLabel = computed(() => {
  if (stats.value) {
    const s = stats.value
    return `${s.done}/${s.total} done`
  }
  return '…'
})

const pageLabel = computed(() => {
  if (total.value === 0) return '0 items'
  const from = offset.value + 1
  const to = Math.min(offset.value + pageSize, total.value)
  return `${from}–${to} / ${total.value}`
})

const canPrev = computed(() => offset.value > 0)
const canNext = computed(() => offset.value + pageSize < total.value)

function onToggle(e: Event) {
  const open = (e.target as HTMLDetailsElement).open
  emit('update:open', open)
}

function statusIcon(status: string) {
  switch (status) {
    case 'done': return CheckCircle2
    case 'in_progress': return Loader2
    case 'failed': return XCircle
    case 'cancelled': return Ban
    default: return Circle
  }
}

function statusClass(status: string): string {
  switch (status) {
    case 'done': return 'text-success'
    case 'in_progress': return 'text-accent animate-spin'
    case 'failed': return 'text-danger'
    case 'cancelled': return 'text-muted'
    default: return 'text-muted'
  }
}
</script>

<template>
  <details class="wi-batch group" :open="open" @toggle="onToggle">
    <summary
      class="cursor-pointer select-none flex items-center gap-2 text-[11px] text-muted py-1 list-none hover:text-foreground transition"
    >
      <span class="truncate flex-1">清单 · {{ batchTitle }}</span>
      <span class="shrink-0 tabular-nums">{{ statsLabel }}</span>
    </summary>
    <div class="pl-1 pb-1 space-y-1">
      <div v-if="loading" class="text-[11px] text-muted py-1">加载中…</div>
      <div v-else-if="error" class="text-[11px] text-danger py-1">{{ error }}</div>
      <template v-else>
        <div
          v-for="row in items"
          :key="row.id"
          class="flex items-start gap-2 text-[11px] py-0.5 min-h-[1.25rem]"
        >
          <component
            :is="statusIcon(row.status)"
            class="w-3 h-3 shrink-0 mt-0.5"
            :class="statusClass(row.status)"
          />
          <div class="min-w-0 flex-1">
            <div class="text-foreground truncate">{{ row.title }}</div>
            <div v-if="row.resultSummary" class="text-muted truncate">{{ row.resultSummary }}</div>
          </div>
        </div>
        <div v-if="!items.length" class="text-[11px] text-muted py-1">暂无条目</div>
        <div
          v-if="total > pageSize"
          class="flex items-center justify-between gap-2 pt-1 text-[10px] text-muted"
        >
          <span class="tabular-nums">{{ pageLabel }}</span>
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="p-0.5 rounded hover:bg-hover disabled:opacity-40"
              :disabled="!canPrev"
              aria-label="上一页"
              @click.stop="prevPage"
            >
              <ChevronLeft class="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              class="p-0.5 rounded hover:bg-hover disabled:opacity-40"
              :disabled="!canNext"
              aria-label="下一页"
              @click.stop="nextPage"
            >
              <ChevronRight class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </template>
    </div>
  </details>
</template>

<style scoped>
.wi-batch > summary::-webkit-details-marker { display: none; }
</style>
