<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, ChevronRight, FileCode2 } from 'lucide-vue-next'
import type { ToolCall } from '../../types/chat'
import { buildFileChangeSummaries, workspaceRelativeDisplayPath } from '../../lib/toolCallDisplay'
import DiffView from './DiffView.vue'
import { useChatStore } from '../../stores/chat'

const props = defineProps<{
  toolCalls: ToolCall[]
}>()

const chat = useChatStore()
const drawerOpen = ref(false)
const reviewedPath = ref<string | null>(null)
const activeOperationIndex = ref(0)
const changes = computed(() => buildFileChangeSummaries(props.toolCalls))
const reviewedChange = computed(() =>
  changes.value.find(change => change.path === reviewedPath.value) ?? null
)
const activeDiff = computed(() => {
  const diffs = reviewedChange.value?.diffs
  if (!diffs?.length) return null
  return diffs[Math.min(activeOperationIndex.value, diffs.length - 1)]
})
const totals = computed(() =>
  changes.value.reduce(
    (sum, change) => ({ adds: sum.adds + change.adds, dels: sum.dels + change.dels }),
    { adds: 0, dels: 0 }
  )
)

function displayPath(path: string): string {
  return workspaceRelativeDisplayPath(path, chat.current?.workspaceRoot)
}

function toggleDrawer(): void {
  drawerOpen.value = !drawerOpen.value
  if (!drawerOpen.value) {
    reviewedPath.value = null
    activeOperationIndex.value = 0
  }
}

function toggleReview(path: string): void {
  if (reviewedPath.value === path) {
    reviewedPath.value = null
    activeOperationIndex.value = 0
    return
  }

  const change = changes.value.find(item => item.path === path)
  reviewedPath.value = path
  activeOperationIndex.value = Math.max((change?.diffs.length ?? 1) - 1, 0)
}

function selectOperation(index: number): void {
  activeOperationIndex.value = index
}
</script>

<template>
  <section
    v-if="changes.length"
    class="change-summary mb-2 overflow-hidden rounded-xl border border-border panel-elevated shadow-sm"
  >
    <button
      type="button"
      class="change-summary-drawer-toggle"
      :aria-expanded="drawerOpen"
      @click="toggleDrawer"
    >
      <FileCode2 class="h-3.5 w-3.5 shrink-0 text-muted" />
      <span class="text-xs font-medium text-foreground/85">会话修改</span>
      <span class="min-w-0 flex-1 truncate text-left text-[11px] text-muted">
        {{ changes.length }} 个文件
      </span>
      <span class="shrink-0 text-[11px] text-green-500 tabular-nums">+{{ totals.adds }}</span>
      <span class="shrink-0 text-[11px] text-red-500 tabular-nums">-{{ totals.dels }}</span>
      <ChevronDown
        class="h-3.5 w-3.5 shrink-0 text-muted transition-transform duration-200"
        :class="drawerOpen ? 'rotate-180' : ''"
      />
    </button>

    <div v-if="drawerOpen" class="change-summary-files">
      <section v-for="change in changes" :key="change.path" class="change-summary-file">
        <button
          type="button"
          class="change-summary-file-toggle"
          :aria-expanded="reviewedPath === change.path"
          @click="toggleReview(change.path)"
        >
          <component
            :is="reviewedPath === change.path ? ChevronDown : ChevronRight"
            class="h-3 w-3 shrink-0 text-muted"
          />
          <span class="min-w-0 flex-1 truncate text-left font-mono" :title="displayPath(change.path)">
            {{ displayPath(change.path) }}
          </span>
          <span class="shrink-0 text-green-500 tabular-nums">+{{ change.adds }}</span>
          <span class="shrink-0 text-red-500 tabular-nums">-{{ change.dels }}</span>
          <span class="shrink-0 text-[10px] text-muted">Review</span>
        </button>

        <div v-if="reviewedPath === change.path && activeDiff" class="change-review-body">
          <div
            v-if="change.diffs.length > 1"
            class="change-operation-tabs"
            role="tablist"
            aria-label="文件修改操作"
          >
            <button
              v-for="(diff, diffIndex) in change.diffs"
              :id="`operation-tab-${diff.toolCallId}`"
              :key="diff.toolCallId"
              type="button"
              role="tab"
              class="change-operation-tab"
              :class="activeOperationIndex === diffIndex ? 'is-active' : ''"
              :aria-selected="activeOperationIndex === diffIndex"
              :aria-controls="`operation-panel-${diff.toolCallId}`"
              @click="selectOperation(diffIndex)"
            >
              操作 {{ diffIndex + 1 }}
            </button>
            <span class="change-operation-count">{{ change.diffs.length }} 次操作</span>
          </div>

          <div
            :id="`operation-panel-${activeDiff.toolCallId}`"
            role="tabpanel"
            :aria-labelledby="change.diffs.length > 1 ? `operation-tab-${activeDiff.toolCallId}` : undefined"
          >
            <DiffView
              :key="activeDiff.toolCallId"
              :diff-lines="activeDiff.diffLines"
              :diff-stats="activeDiff.diffStats"
            />
          </div>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.change-summary-drawer-toggle {
  min-height: 38px;
  @apply flex w-full cursor-pointer items-center gap-2 px-3 text-left transition-colors hover:bg-hover/60;
}
.change-summary-files {
  max-height: min(42vh, 22rem);
  @apply divide-y divide-border overflow-y-auto border-t border-border bg-accent-muted/10;
}
.change-summary-file-toggle {
  min-height: 34px;
  @apply flex w-full cursor-pointer items-center gap-2 px-3 text-[11px] text-muted transition-colors hover:bg-hover/60 hover:text-foreground/85;
}
.change-review-body {
  max-height: min(48vh, 28rem);
  @apply space-y-2 overflow-y-auto border-t border-border/70 bg-background px-2 py-2;
}
.change-operation-tabs {
  min-height: 32px;
  scrollbar-width: thin;
  @apply sticky top-0 z-10 flex items-end overflow-x-auto border-b border-border bg-background px-1;
}
.change-operation-tab {
  min-width: max-content;
  @apply relative shrink-0 cursor-pointer px-3 py-2 text-[11px] text-muted transition-colors hover:text-foreground/85;
}
.change-operation-tab::after {
  content: '';
  height: 2px;
  @apply absolute inset-x-2 bottom-0 scale-x-0 bg-accent transition-transform;
}
.change-operation-tab.is-active {
  @apply font-medium text-foreground;
}
.change-operation-tab.is-active::after {
  @apply scale-x-100;
}
.change-operation-count {
  @apply sticky right-0 ml-auto min-w-max shrink-0 bg-background px-2 py-2 text-[10px] text-muted;
}
</style>
