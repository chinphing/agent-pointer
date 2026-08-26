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
    <div v-if="!singleFile" class="change-summary-sizer" aria-hidden="true">
      <div
        v-for="change in files"
        :key="`sizer-${change.path}`"
        class="change-summary-sizer-row"
      >
        <span>{{ displayPath(change.path) }}</span>
        <span v-if="change.adds > 0">+{{ change.adds }}</span>
        <span v-if="change.dels > 0">-{{ change.dels }}</span>
      </div>
    </div>
    <button
      type="button"
      class="change-summary-header"
      :aria-expanded="singleFile ? undefined : expanded"
      :title="singleFile ? displayPath(singleFile.path) : undefined"
      @click="onHeaderClick"
    >
      <FileText class="h-3.5 w-3.5 shrink-0 text-muted" />
      <span class="ellipsis-start min-w-0 text-[13px] text-muted">{{ headerLabel }}&lrm;</span>
      <span
        v-if="totals.adds > 0"
        class="shrink-0 text-[11px] tabular-nums text-green-500"
      >+{{ totals.adds }}</span>
      <span
        v-if="totals.dels > 0"
        class="shrink-0 text-[11px] tabular-nums text-red-500"
      >-{{ totals.dels }}</span>
      <ChevronDown
        v-if="!singleFile"
        class="ml-auto h-3.5 w-3.5 shrink-0 text-muted transition-transform duration-200"
        :class="expanded ? 'rotate-180' : ''"
      />
    </button>

    <div
      v-if="!singleFile"
      class="change-summary-files"
      :class="{ 'is-collapsed': !expanded }"
      :aria-hidden="!expanded"
      :inert="!expanded"
    >
      <button
        v-for="change in files"
        :key="change.path"
        type="button"
        class="change-summary-file"
        :title="displayPath(change.path)"
        @click="openFile(change.path)"
      >
        <span class="ellipsis-start min-w-0 flex-1 font-mono">{{ displayPath(change.path) }}&lrm;</span>
        <span
          v-if="change.adds > 0"
          class="shrink-0 text-green-500 tabular-nums"
        >+{{ change.adds }}</span>
        <span
          v-if="change.dels > 0"
          class="shrink-0 text-red-500 tabular-nums"
        >-{{ change.dels }}</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
.change-summary {
  display: grid;
  grid-template-columns: minmax(0, max-content);
  width: max-content;
  max-width: 100%;
  min-width: 0;
  @apply overflow-hidden rounded-lg border border-border/80 bg-card;
}
.change-summary-sizer {
  grid-column: 1;
  height: 0;
  overflow: hidden;
  visibility: hidden;
  pointer-events: none;
}
.change-summary-sizer-row {
  display: flex;
  width: max-content;
  align-items: center;
  gap: 0.5rem;
  padding: 0 0.75rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  white-space: nowrap;
}
.change-summary-header {
  grid-column: 1;
  min-width: 0;
  min-height: 34px;
  @apply flex w-full items-center gap-2 px-3 text-left transition-colors hover:bg-hover/40;
}
.change-summary-files {
  grid-column: 1;
  min-width: 0;
  max-height: 12rem;
  overflow-x: hidden;
  overflow-y: auto;
  @apply divide-y divide-border/80 border-t border-border/80;
}
.change-summary-files.is-collapsed {
  max-height: 0;
  overflow: hidden;
  border-top-width: 0;
}
.change-summary-file {
  min-height: 32px;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  box-sizing: border-box;
  @apply flex cursor-pointer items-center gap-2 px-3 text-[12px] text-muted transition-colors hover:bg-hover/40 hover:text-foreground/85;
}
</style>
