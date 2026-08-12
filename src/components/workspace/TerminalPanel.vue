<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Columns2, Copy, FolderPlus, Loader2, RotateCcw, Rows2, SquareTerminal, X } from 'lucide-vue-next'
import { useConsoleStore, type WorkspaceConsoleTab } from '../../stores/console'
import {
  collectLeaves,
  createLeaf,
  findLeaf,
  findLeafByTabId,
  firstLeafWithTab,
  leafCount,
  MAX_PANES,
  removeLeaf,
  setLeafTabId,
  splitLeaf,
  type TerminalPaneDirection,
  type TerminalPaneNode
} from '../../lib/terminalLayout'
import TerminalSplitPane from './TerminalSplitPane.vue'

const props = defineProps<{ workspaceRoot: string; conversationId: string; active: boolean }>()

const consoleStore = useConsoleStore()
const error = ref('')
const loading = ref(false)
const contextMenu = ref<{ tab: WorkspaceConsoleTab; x: number; y: number } | null>(null)
/** 拆分布局树；null = 空态（无会话）。 */
const layout = ref<TerminalPaneNode | null>(null)
const focusedPaneId = ref<string | null>(null)

const workspaceTabs = computed(() =>
  consoleStore.tabs.filter(tab =>
    tab.workspaceRoot === props.workspaceRoot &&
    tab.conversationId === props.conversationId
  )
)
const activeTab = computed(() =>
  workspaceTabs.value.find(tab => tab.id === consoleStore.activeSessionId) ?? null
)
const hasWorkspace = computed(() => !!props.workspaceRoot.trim())
const paneCount = computed(() => leafCount(layout.value))
const canSplit = computed(() => !!layout.value && paneCount.value < MAX_PANES && !loading.value)

/** 焦点窗格（缺省回退到第一个有会话的窗格）。 */
const activePaneLeaf = computed(() => {
  if (!layout.value) return null
  return findLeaf(layout.value, focusedPaneId.value ?? '') ?? firstLeafWithTab(layout.value)
})
const activePaneTab = computed(() => {
  const id = activePaneLeaf.value?.tabId
  if (!id) return null
  return workspaceTabs.value.find(tab => tab.id === id) ?? null
})

let opening: Promise<void> | null = null
let removeOutsideMenuListeners: (() => void) | null = null

function showError(cause: unknown) {
  error.value = cause instanceof Error ? cause.message : String(cause)
}

function focusPane(leafId: string | null) {
  if (!leafId) return
  focusedPaneId.value = leafId
  const leaf = layout.value ? findLeaf(layout.value, leafId) : null
  if (leaf?.tabId) consoleStore.select(leaf.tabId)
}

/** 创建会话并显示在目标窗格（缺省 = 焦点窗格或第一个有会话的窗格；无窗格则建根窗格）。 */
async function createTab(cwd?: string): Promise<WorkspaceConsoleTab | null> {
  if (!hasWorkspace.value) return null
  if (opening) {
    await opening
    return null
  }
  let created: WorkspaceConsoleTab | null = null
  opening = (async () => {
    loading.value = true
    error.value = ''
    try {
      const tab = await consoleStore.create(props.workspaceRoot, props.conversationId, 80, 24, cwd)
      created = tab
      const target =
        (layout.value ? findLeaf(layout.value, focusedPaneId.value ?? '') : null) ??
        firstLeafWithTab(layout.value)
      if (target && layout.value) {
        layout.value = setLeafTabId(layout.value, target.id, tab.id)
        focusPane(target.id)
      } else {
        layout.value = createLeaf(tab.id)
        focusPane(layout.value.id)
      }
    } catch (cause) {
      showError(cause)
    } finally {
      loading.value = false
      opening = null
    }
  })()
  await opening
  return created
}

/** 把焦点窗格（或第一个有会话的窗格）拆成两个；新窗格绑定一个新会话。 */
async function splitPane(direction: TerminalPaneDirection) {
  if (!layout.value || !canSplit.value || opening) return
  opening = (async () => {
    loading.value = true
    error.value = ''
    try {
      const target = findLeaf(layout.value!, focusedPaneId.value ?? '') ?? firstLeafWithTab(layout.value!)
      if (!target) return
      const tab = await consoleStore.create(props.workspaceRoot, props.conversationId, 80, 24)
      const { node, newLeaf } = splitLeaf(layout.value!, target.id, direction)
      layout.value = setLeafTabId(node, newLeaf.id, tab.id)
      consoleStore.select(tab.id)
      focusPane(newLeaf.id)
    } catch (cause) {
      showError(cause)
    } finally {
      loading.value = false
      opening = null
    }
  })()
  await opening
}

/** 关闭窗格：关会话并从布局移除；分支只剩一个子节点时折叠。 */
async function closePane(leafId: string) {
  if (!layout.value) return
  const { node, removed } = removeLeaf(layout.value, leafId)
  if (removed?.tabId) {
    try {
      await consoleStore.close(removed.tabId)
    } catch (cause) {
      showError(cause)
    }
  }
  layout.value = node
  if (!node) {
    focusedPaneId.value = null
    return
  }
  const leaves = collectLeaves(node)
  const next = leaves.find(leaf => leaf.id === focusedPaneId.value) ?? leaves[0] ?? null
  focusPane(next?.id ?? null)
}

/** 关闭会话（tab bar X / 右键菜单）：绑定了窗格则连窗格一起关。 */
async function closeTab(tabId: string) {
  error.value = ''
  const bound = layout.value ? findLeafByTabId(layout.value, tabId) : null
  if (bound) {
    await closePane(bound.id)
    return
  }
  try {
    await consoleStore.close(tabId)
  } catch (cause) {
    showError(cause)
  }
}

/** tab bar 点击：已在某窗格 → 聚焦；否则替换焦点窗格的会话。 */
function selectTab(id: string) {
  if (!layout.value) {
    layout.value = createLeaf(id)
    focusPane(layout.value.id)
    return
  }
  const bound = findLeafByTabId(layout.value, id)
  if (bound) {
    focusPane(bound.id)
    return
  }
  const target = findLeaf(layout.value, focusedPaneId.value ?? '') ?? firstLeafWithTab(layout.value)
  if (target) {
    layout.value = setLeafTabId(layout.value, target.id, id)
    focusPane(target.id)
  }
}

async function restartActiveTab() {
  const leaf = activePaneLeaf.value
  const tab = leaf?.tabId ? workspaceTabs.value.find(item => item.id === leaf.tabId) : null
  if (!leaf || !tab) return
  error.value = ''
  loading.value = true
  try {
    const fresh = await consoleStore.create(props.workspaceRoot, props.conversationId, 80, 24, tab.cwd)
    await consoleStore.close(tab.id)
    // 同步重新绑定：workspaceTabs 的失效清理 watch 之后才跑，此时 leaf.tabId
    // 已指向新会话，不会被当作失效窗格删除。
    const target = layout.value ? findLeaf(layout.value, leaf.id) : null
    if (target) {
      layout.value = setLeafTabId(layout.value!, target.id, fresh.id)
      focusPane(target.id)
    } else {
      layout.value = createLeaf(fresh.id)
      focusPane(layout.value.id)
    }
  } catch (cause) {
    showError(cause)
  } finally {
    loading.value = false
  }
}

function openContextMenu(event: MouseEvent, tab: WorkspaceConsoleTab) {
  contextMenu.value = { tab, x: event.clientX, y: event.clientY }
}

function closeContextMenu() {
  contextMenu.value = null
}

function installOutsideMenuListeners() {
  removeOutsideMenuListeners?.()
  const closeWhenOutside = (event: Event) => {
    const target = event.target as HTMLElement | null
    if (!target?.closest('.console-context-menu, .console-tab')) closeContextMenu()
  }
  const closeOnEscape = (event: KeyboardEvent) => {
    if (event.key === 'Escape') closeContextMenu()
  }
  window.addEventListener('pointerdown', closeWhenOutside, true)
  window.addEventListener('keydown', closeOnEscape)
  removeOutsideMenuListeners = () => {
    window.removeEventListener('pointerdown', closeWhenOutside, true)
    window.removeEventListener('keydown', closeOnEscape)
  }
}

async function copyPath(tab: WorkspaceConsoleTab) {
  try {
    await navigator.clipboard.writeText(tab.cwd)
  } catch (cause) {
    showError(cause)
  } finally {
    closeContextMenu()
  }
}

watch(contextMenu, menu => {
  if (menu) installOutsideMenuListeners()
  else removeOutsideMenuListeners?.()
})

watch(() => props.workspaceRoot, () => {
  layout.value = null
  focusedPaneId.value = null
  error.value = ''
  if (props.workspaceRoot && props.active) void createTab()
})

watch(() => props.conversationId, () => {
  // 切换会话：布局重置为该会话现有 tab；没有则新建。
  const tabs = workspaceTabs.value
  const existing = tabs.find(tab => tab.id === consoleStore.activeSessionId) ?? tabs[0] ?? null
  if (existing) {
    layout.value = createLeaf(existing.id)
    focusPane(layout.value.id)
  } else if (props.workspaceRoot && props.active) {
    void createTab()
  } else {
    layout.value = null
    focusedPaneId.value = null
  }
})

// 会话被外部关闭（closeWorkspace / 其他入口）→ 移除失效窗格，分支折叠。
watch(workspaceTabs, (tabs) => {
  const node = layout.value
  if (!node) return
  const ids = new Set(tabs.map(tab => tab.id))
  const deadTabIds = collectLeaves(node)
    .filter(leaf => leaf.tabId !== null && !ids.has(leaf.tabId as string))
    .map(leaf => leaf.tabId as string)
  if (deadTabIds.length === 0) return
  let current: TerminalPaneNode | null = node
  for (const tabId of deadTabIds) {
    if (!current) break
    const leaf = findLeafByTabId(current, tabId)
    if (!leaf) continue
    current = removeLeaf(current, leaf.id).node
  }
  layout.value = current
  if (!current) {
    focusedPaneId.value = null
    return
  }
  const leaves = collectLeaves(current)
  if (!leaves.some(leaf => leaf.id === focusedPaneId.value)) {
    focusPane(leaves[0]?.id ?? null)
  }
})

onMounted(() => {
  if (!props.active) return
  void (async () => {
    const existing = activeTab.value ?? workspaceTabs.value[0] ?? null
    if (existing) {
      layout.value = createLeaf(existing.id)
      focusPane(layout.value.id)
    } else {
      await createTab()
    }
  })()
})

onBeforeUnmount(() => {
  removeOutsideMenuListeners?.()
  removeOutsideMenuListeners = null
})
</script>

<template>
  <section class="terminal-panel relative flex min-h-0 flex-1 flex-col bg-card text-foreground" @click.self="closeContextMenu">
    <header class="console-chrome flex h-8 shrink-0 items-center gap-1 border-b border-border px-2 text-[11px]">
      <SquareTerminal class="h-3.5 w-3.5 shrink-0 text-accent" />
      <div class="console-tabs min-w-0 flex-1" role="tablist" aria-label="终端标签">
        <button
          v-for="tab in workspaceTabs"
          :key="tab.id"
          type="button"
          role="tab"
          class="console-tab"
          :class="consoleStore.activeSessionId === tab.id && 'is-active'"
          :aria-selected="consoleStore.activeSessionId === tab.id"
          :title="tab.cwd"
          @mousedown.left.prevent="selectTab(tab.id)"
          @contextmenu.prevent="openContextMenu($event, tab)"
        >
          <span class="truncate">{{ tab.label }}</span>
          <span v-if="tab.exited" class="text-amber-300">•</span>
          <X class="h-3 w-3 shrink-0 opacity-60 hover:opacity-100" @click.stop="closeTab(tab.id)" />
        </button>
        <button
          v-if="hasWorkspace"
          type="button"
          class="console-new-tab"
          title="新建独立 Shell"
          aria-label="新建独立 Shell"
          :disabled="loading"
          @click="createTab()"
        ><FolderPlus class="h-3.5 w-3.5" /></button>
      </div>
      <span v-if="loading" class="flex items-center gap-1 text-slate-400"><Loader2 class="h-3 w-3 animate-spin" />启动中</span>
      <button type="button" class="terminal-action" title="左右拆分窗格" aria-label="左右拆分窗格" :disabled="!canSplit" @click="splitPane('row')"><Columns2 class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" title="上下拆分窗格" aria-label="上下拆分窗格" :disabled="!canSplit" @click="splitPane('column')"><Rows2 class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" title="重启当前 Shell" :disabled="loading || !activePaneTab" @click="restartActiveTab"><RotateCcw class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" title="结束当前 Shell" :disabled="loading || !activePaneTab" @click="activePaneLeaf && closePane(activePaneLeaf.id)"><X class="h-3.5 w-3.5" /></button>
    </header>
    <div
      v-if="contextMenu"
      class="console-context-menu"
      role="menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
    >
      <button type="button" role="menuitem" @click="copyPath(contextMenu.tab)"><Copy class="h-3.5 w-3.5" />复制完整路径</button>
      <button type="button" role="menuitem" @click="closeTab(contextMenu.tab.id); closeContextMenu()"><X class="h-3.5 w-3.5" />关闭 Shell</button>
    </div>
    <p v-if="!hasWorkspace" class="m-auto max-w-56 text-center text-xs text-slate-400">请先在输入区选择项目目录，再启动调试终端。</p>
    <div v-else-if="!layout && !loading" class="m-auto flex flex-col items-center gap-3 text-center text-xs text-slate-400">
      <p>新建一个独立 Shell；每个标签有自己的目录、环境和前台进程。</p>
      <button type="button" class="console-create-first" @click="createTab()"><FolderPlus class="h-3.5 w-3.5" />新建终端</button>
    </div>
    <p v-if="error" class="absolute inset-x-3 top-11 z-10 rounded border border-red-400/40 bg-red-950/90 p-2 text-xs text-red-200">{{ error }}</p>
    <div
      v-show="hasWorkspace && layout"
      class="terminal-layout min-h-0 flex-1 p-2"
      data-workspace-terminal
    >
      <TerminalSplitPane
        v-if="layout"
        :node="layout"
        :workspace-root="props.workspaceRoot"
        :conversation-id="props.conversationId"
        :focused-pane-id="focusedPaneId"
        @focus="focusPane"
        @close="closePane"
      />
    </div>
  </section>
</template>

<style scoped>
.terminal-layout { @apply flex min-h-0 flex-1; }
.console-chrome,
.console-tabs,
.console-tab,
.console-new-tab,
.terminal-action { @apply select-none; }
.console-tabs { @apply flex min-w-0 items-center gap-1 overflow-x-auto; }
.console-tab { @apply flex max-w-32 items-center gap-1 rounded px-1.5 py-1 text-muted; }
.console-tab:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-tab.is-active { background: hsl(var(--accent-muted)); color: hsl(var(--accent)); }
.console-new-tab, .terminal-action { @apply shrink-0 rounded p-1 text-muted disabled:cursor-not-allowed disabled:opacity-40; }
.console-new-tab:hover, .terminal-action:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-context-menu { @apply fixed z-20 min-w-36 rounded border p-1 shadow-lg; border-color: hsl(var(--border)); background: hsl(var(--card)); transform: translateY(2px); }
.console-context-menu button { @apply flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground; }
.console-context-menu button:hover { background: hsl(var(--hover)); }
.console-create-first { @apply flex items-center gap-1 rounded border px-2 py-1.5 text-foreground; border-color: hsl(var(--border)); }
.console-create-first:hover { background: hsl(var(--hover)); }
</style>
