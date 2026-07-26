<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, FileCode2 } from 'lucide-vue-next'
import type { ChatMessage } from '../../types/chat'
import { lastTurnFileChanges } from '../../lib/lastTurnFileChanges'
import { workspaceRelativeDisplayPath } from '../../lib/toolCallDisplay'
import { useChatStore } from '../../stores/chat'
import { useWorkspacePanelStore } from '../../stores/workspacePanel'

const props = defineProps<{
  messages: ChatMessage[]
}>()

const chat = useChatStore()
const workspacePanel = useWorkspacePanelStore()
const drawerOpen = ref(false)

const turnChanges = computed(() => lastTurnFileChanges(props.messages))
const files = computed(() => turnChanges.value?.files ?? [])

function displayPath(path: string): string {
  return workspaceRelativeDisplayPath(path, chat.current?.workspaceRoot)
}

function toggleDrawer(): void {
  drawerOpen.value = !drawerOpen.value
}

function openFile(path: string): void {
  const turnId = turnChanges.value?.turnId
  const conversationId = chat.current?.id
  const workspaceRoot = chat.current?.workspaceRoot?.trim()
  if (!turnId || !conversationId || !workspaceRoot) {
    console.warn('[ChangeSummary] cannot open turn diff', {
      turnId,
      conversationId,
      hasWorkspace: Boolean(workspaceRoot),
      path
    })
    return
  }
  workspacePanel.openTurnDiff({
    conversationId,
    turnId,
    path
  })
}
</script>

<template>
  <section
    v-if="files.length"
    class="change-summary mb-2 overflow-hidden rounded-xl border border-border panel-elevated shadow-sm"
  >
    <button
      type="button"
      class="change-summary-header"
      :aria-expanded="drawerOpen"
      @click="toggleDrawer"
    >
      <FileCode2 class="h-3.5 w-3.5 shrink-0 text-muted" />
      <span class="text-xs font-medium text-foreground/85">本轮修改</span>
      <span class="min-w-0 flex-1 truncate text-left text-[11px] text-muted">
        {{ files.length }} 个文件
      </span>
      <ChevronDown
        class="h-3.5 w-3.5 shrink-0 text-muted transition-transform duration-200"
        :class="drawerOpen ? 'rotate-180' : ''"
      />
    </button>

    <div v-if="drawerOpen" class="change-summary-files">
      <button
        v-for="change in files"
        :key="change.path"
        type="button"
        class="change-summary-file-toggle"
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
.change-summary-header {
  min-height: 38px;
  @apply flex w-full items-center gap-2 px-3 text-left transition-colors hover:bg-hover/40;
}
.change-summary-files {
  max-height: min(28vh, 12rem);
  @apply divide-y divide-border overflow-y-auto border-t border-border bg-accent-muted/10;
}
.change-summary-file-toggle {
  min-height: 34px;
  @apply flex w-full cursor-pointer items-center gap-2 px-3 text-[11px] text-muted transition-colors hover:bg-hover/60 hover:text-foreground/85;
}
</style>
