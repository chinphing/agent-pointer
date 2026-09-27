<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { MIN_PANE_RATIO, type TerminalPaneBranch, type TerminalPaneNode } from '../../lib/terminalLayout'
import TerminalPane from './TerminalPane.vue'


const { t } = useI18n()
const props = defineProps<{
  node: TerminalPaneNode
  workspaceRoot: string
  conversationId: string
  focusedPaneId: string | null
  /** 所属 tab 是否正在显示；隐藏时窗格仍挂载，但不向 PTY 推 0 尺寸。 */
  visible: boolean
}>()

const emit = defineEmits<{ focus: [leafId: string]; close: [leafId: string] }>()

/** 分割条拖拽：按容器总长换算百分比，更新分支的比例（最小值 MIN_PANE_RATIO）。 */
function startDrag(event: PointerEvent, node: TerminalPaneBranch) {
  const isRow = node.direction === 'row'
  const container = (event.currentTarget as HTMLElement).parentElement
  if (!container) return
  const startPos = isRow ? event.clientX : event.clientY
  const [a0] = node.sizes
  const total = isRow ? container.clientWidth : container.clientHeight
  const onMove = (move: PointerEvent) => {
    if (total <= 0) return
    const delta = (isRow ? move.clientX : move.clientY) - startPos
    const pct = (delta / total) * 100
    const min = MIN_PANE_RATIO * 100
    const a = Math.min(Math.max(a0 + pct, min), 100 - min)
    node.sizes = [a, 100 - a]
  }
  const onUp = () => {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
  }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
}
</script>

<template>
  <TerminalPane
    v-if="node.kind === 'leaf'"
    :workspace-root="workspaceRoot"
    :conversation-id="conversationId"
    :tab-id="node.tabId"
    :focused="focusedPaneId === node.id"
    :visible="visible"
    class="terminal-pane-slot"
    @focus="emit('focus', node.id)"
    @close="emit('close', node.id)"
  />
  <div v-else class="terminal-split" :class="node.direction === 'row' ? 'is-row' : 'is-column'">
    <div class="terminal-pane-slot" :style="{ flexGrow: node.sizes[0], flexBasis: '0' }">
      <TerminalSplitPane
        :node="node.children[0]"
        :workspace-root="workspaceRoot"
        :conversation-id="conversationId"
        :focused-pane-id="focusedPaneId"
        :visible="visible"
        @focus="emit('focus', $event)"
        @close="emit('close', $event)"
      />
    </div>
    <div
      class="terminal-splitter"
      :class="node.direction === 'row' ? 'is-row' : 'is-column'"
      :title="node.direction === 'row' ? t('workspace.terminal.resizeRowTitle') : t('workspace.terminal.resizeColumnTitle')"
      @pointerdown.prevent="startDrag($event, node)"
    />
    <div class="terminal-pane-slot" :style="{ flexGrow: node.sizes[1], flexBasis: '0' }">
      <TerminalSplitPane
        :node="node.children[1]"
        :workspace-root="workspaceRoot"
        :conversation-id="conversationId"
        :focused-pane-id="focusedPaneId"
        :visible="visible"
        @focus="emit('focus', $event)"
        @close="emit('close', $event)"
      />
    </div>
  </div>
</template>

<style scoped>
.terminal-split { @apply flex h-full min-h-0 min-w-0 flex-1; }
.terminal-split.is-row { flex-direction: row; }
.terminal-split.is-column { flex-direction: column; }
.terminal-pane-slot { @apply flex h-full min-h-0 min-w-0 flex-1; }
.terminal-pane-slot > * { @apply h-full min-h-0 min-w-0 flex-1; }
.terminal-splitter {
  @apply shrink-0 select-none;
  background: hsl(var(--border));
  z-index: 1;
  transition: background 120ms ease;
}
.terminal-splitter.is-row { width: 4px; cursor: col-resize; }
.terminal-splitter.is-column { height: 4px; cursor: row-resize; }
.terminal-splitter:hover { background: hsl(var(--accent)); }
</style>
