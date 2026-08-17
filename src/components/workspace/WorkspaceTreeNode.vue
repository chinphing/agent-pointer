<script setup lang="ts">
import { ChevronDown, ChevronRight, File, Folder, FolderOpen, Loader2 } from 'lucide-vue-next'
import type { WorkspaceEntry } from '../../lib/api'

export type WorkspaceTreeNodeModel = WorkspaceEntry & {
  children?: WorkspaceTreeNodeModel[]
  expanded?: boolean
  loading?: boolean
}

const props = defineProps<{
  node: WorkspaceTreeNodeModel
  depth?: number
  /** Path of the row that should stay highlighted (e.g. open context menu). */
  highlightedPath?: string
}>()
const emit = defineEmits<{
  (e: 'toggle', node: WorkspaceTreeNodeModel): void
  (e: 'activate', node: WorkspaceTreeNodeModel): void
  (e: 'contextmenu', event: MouseEvent, node: WorkspaceTreeNodeModel): void
}>()

function activate(node: WorkspaceTreeNodeModel) {
  if (node.kind === 'directory') emit('toggle', node)
  else if (node.kind === 'file') emit('activate', node)
}

function toggle(node: WorkspaceTreeNodeModel) {
  emit('toggle', node)
}

function forwardActivate(node: WorkspaceTreeNodeModel) {
  emit('activate', node)
}

function openContextMenu(event: MouseEvent, node: WorkspaceTreeNodeModel) {
  emit('contextmenu', event, node)
}
</script>

<template>
  <div>
    <button
      class="tree-row"
      :class="highlightedPath === node.path && 'is-selected'"
      :style="{ paddingLeft: `${12 + (depth ?? 0) * 16}px` }"
      type="button"
      :data-workspace-tree-path="node.path"
      @click="activate(node)"
      @contextmenu.prevent="openContextMenu($event, node)"
    >
      <component
        :is="node.loading ? Loader2 : node.expanded ? ChevronDown : ChevronRight"
        v-if="node.kind === 'directory'"
        class="w-3.5 h-3.5 shrink-0"
        :class="node.loading && 'animate-spin'"
      />
      <span v-else class="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
      <component
        :is="node.kind === 'directory' ? (node.expanded ? FolderOpen : Folder) : File"
        class="w-3.5 h-3.5 shrink-0 text-muted"
      />
      <span class="truncate">{{ node.name }}</span>
    </button>
    <WorkspaceTreeNode
      v-for="child in node.expanded ? node.children : []"
      :key="child.path"
      :node="child"
      :depth="(depth ?? 0) + 1"
      :highlighted-path="highlightedPath"
      @toggle="toggle"
      @activate="forwardActivate"
      @contextmenu="openContextMenu"
    />
  </div>
</template>

<style scoped>
.tree-row { @apply w-full flex items-center gap-1.5 pr-3 py-1.5 text-left text-xs hover:bg-hover disabled:cursor-default select-none; }
.tree-row.is-selected { @apply bg-hover text-foreground; }
</style>
