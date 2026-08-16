<script setup lang="ts">
import { computed } from 'vue'
import { ChevronDown, FileText } from 'lucide-vue-next'
import type { FileChangeSummary } from '../../lib/toolCallDisplay'
import { workspaceRelativeDisplayPath } from '../../lib/toolCallDisplay'
import { useChatStore } from '../../stores/chat'
import { useWorkspacePanelStore } from '../../stores/workspacePanel'

const props = defineProps<{
  turnId: string
  files: FileChangeSummary[]
  expanded: boolean
}>()

const emit = defineEmits<{
  toggle: []
}>()

const chat = useChatStore()
const workspacePanel = useWorkspacePanelStore()

const singleFile = computed(() => (props.files.length === 1 ? props.files[0] : null))

const headerLabel = computed(() => {
  if (singleFile.value) {
    return `修改了 ${singleFile.value.fileName}`
  }
  return `修改了 ${props.files.length} 个文件`
})

const totals = computed(() => {
  let adds = 0
  let dels = 0
  for (const file of props.files) {
    adds += file.adds
    dels += file.dels
  }
  return { adds, dels }
})

const showTotals = computed(() => totals.value.adds > 0 || totals.value.dels > 0)

function displayPath(path: string): string {
  return workspaceRelativeDisplayPath(path, chat.current?.workspaceRoot)
}

function openFile(path: string): void {
  const conversationId = chat.current?.id
  const workspaceRoot = chat.current?.workspaceRoot?.trim()
  if (!props.turnId || !conversationId || !workspaceRoot) {
    console.warn('[ChangeSummary] cannot open turn diff', {
      turnId: props.turnId,
      conversationId,
      hasWorkspace: Boolean(workspaceRoot),
      path
    })
    return
  }
  workspacePanel.openTurnDiff({
    conversationId,
    turnId: props.turnId,
    path
  })
}

function onHeaderClick(): void {
  if (singleFile.value) {
    openFile(singleFile.value.path)
    return
  }
  emit('toggle')
}
</script>

<template>
  <section v-if="files.length" class="change-summary">
    <button
      type="button"
      class="change-summary-header"
      :aria-expanded="singleFile ? undefined : expanded"
      :title="singleFile ? displayPath(singleFile.path) : undefined"
      @click="onHeaderClick"
    >
      <FileText class="h-3.5 w-3.5 shrink-0 text-muted" />
      <span class="min-w-0 truncate text-left text-[13px] text-muted">{{ headerLabel }}</span>
      <span
        v-if="showTotals"
        class="inline-flex shrink-0 items-center gap-1 text-[11px] tabular-nums"
      >
        <span class="text-green-500">+{{ totals.adds }}</span>
        <span class="text-red-500">-{{ totals.dels }}</span>
      </span>
      <ChevronDown
        v-if="!singleFile"
        class="ml-auto h-3.5 w-3.5 shrink-0 text-muted transition-transform duration-200"
        :class="expanded ? 'rotate-180' : ''"
      />
    </button>

    <div v-if="!singleFile && expanded" class="change-summary-files">
      <button
        v-for="change in files"
        :key="change.path"
        type="button"
        class="change-summary-file"
        :title="displayPath(change.path)"
        @click="openFile(change.path)"
      >
        <span class="min-w-0 flex-1 truncate text-left font-mono">
          {{ displayPath(change.path) }}
        </span>
        <span
          v-if="change.adds || change.dels"
          class="shrink-0 text-green-500 tabular-nums"
        >+{{ change.adds }}</span>
        <span
          v-if="change.adds || change.dels"
          class="shrink-0 text-red-500 tabular-nums"
        >-{{ change.dels }}</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
.change-summary {
  min-width: 15.5rem;
  width: max-content;
  max-width: min(22rem, 100%);
  @apply overflow-hidden rounded-lg border border-border/80 bg-card;
}
.change-summary-header {
  min-height: 34px;
  @apply flex w-full items-center gap-2 px-3 text-left transition-colors hover:bg-hover/40;
}
.change-summary-files {
  max-height: 12rem;
  @apply divide-y divide-border/80 overflow-y-auto border-t border-border/80;
}
.change-summary-file {
  min-height: 32px;
  @apply flex w-full cursor-pointer items-center gap-2 px-3 text-[12px] text-muted transition-colors hover:bg-hover/40 hover:text-foreground/85;
}
</style>
