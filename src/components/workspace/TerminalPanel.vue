<script setup lang="ts">
import { computed, onActivated, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

defineOptions({ name: 'WorkspaceTerminalPanel' })
import { Columns2, Copy, FolderPlus, Loader2, RotateCcw, Rows2, X } from 'lucide-vue-next'
import { useConsoleStore, type WorkspaceConsoleTab } from '../../stores/console'
import {
  collectLeaves,
  findLeaf,
  firstLeafWithTab,
  leafCount,
  removeLeaf,
  setLeafTabId,
  splitLeaf,
  type TerminalPaneDirection,
  type TerminalPaneNode
} from '../../lib/terminalLayout'
import {
  activateGroup,
  addGroup,
  createTerminalGroupsState,
  findGroup,
  getActiveGroup,
  rebuildGroupsFromExistingTabs,
  removeGroup,
  setGroupLayout,
  type TerminalGroup,
  type TerminalGroupsState
} from '../../lib/terminalGroups'
import TerminalSplitPane from './TerminalSplitPane.vue'

const props = defineProps<{ workspaceRoot: string; conversationId: string; active: boolean }>()

const { t } = useI18n()
const consoleStore = useConsoleStore()
const error = ref('')
const loading = ref(false)
const contextMenu = ref<{ group: TerminalGroup; x: number; y: number } | null>(null)
/**
 * group（= tab）状态：每个 group 持有一棵独立布局树，切换 tab 恢复该组布局。
 * 各组窗格保持挂载（v-show），避免切 tab 销毁 xterm 并清空屏幕。
 */
const groupsState = ref<TerminalGroupsState>(createTerminalGroupsState())
const focusedPaneId = ref<string | null>(null)

const workspaceTabs = computed(() =>
  consoleStore.tabs.filter(tab =>
    tab.workspaceRoot === props.workspaceRoot &&
    tab.conversationId === props.conversationId
  )
)
const hasWorkspace = computed(() => !!props.workspaceRoot.trim())
/** 当前激活组。 */
const activeGroup = computed(() => getActiveGroup(groupsState.value))
/** 当前显示的布局树（= 激活组的树）。 */
const layout = computed(() => activeGroup.value?.layout ?? null)
/** 拆分数量不设上限；只在无布局/加载中时禁用。 */
const canSplit = computed(() => !!layout.value && !loading.value)

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

let groupSeq = 0
function nextGroupId(): string {
  groupSeq += 1
  return `group-${groupSeq}`
}

let opening: Promise<void> | null = null
let removeOutsideMenuListeners: (() => void) | null = null

function showError(cause: unknown) {
  error.value = cause instanceof Error ? cause.message : String(cause)
}

function focusPane(leafId: string | null) {
  if (!leafId) return
  focusedPaneId.value = leafId
  const group = activeGroup.value
  if (group) group.focusedPaneId = leafId
  const leaf = layout.value ? findLeaf(layout.value, leafId) : null
  if (leaf?.tabId) consoleStore.select(leaf.tabId)
}

/** 组内焦点 session（用于 tab 标签/路径/右键）。 */
function groupFocusedTab(group: TerminalGroup): WorkspaceConsoleTab | null {
  const tree = group.layout
  if (!tree) return null
  const leaf = findLeaf(tree, group.focusedPaneId ?? '') ?? firstLeafWithTab(tree) ?? null
  if (!leaf?.tabId) return null
  return workspaceTabs.value.find(tab => tab.id === leaf.tabId) ?? null
}

/** Tab label: “Terminal N” (hover tip via groupTitle). */
function groupLabel(index: number): string {
  return t('workspace.terminalN', { n: index + 1 })
}

function groupTitle(group: TerminalGroup): string {
  const tab = groupFocusedTab(group)
  if (!tab) return t('workspace.terminalLabel')
  const count = leafCount(group.layout)
  return count > 1 ? t('workspace.panesCount', { cwd: tab.cwd, count }) : tab.cwd
}

function groupHasExited(group: TerminalGroup): boolean {
  const tree = group.layout
  if (!tree) return false
  return collectLeaves(tree).some(leaf => {
    if (!leaf.tabId) return false
    return workspaceTabs.value.find(tab => tab.id === leaf.tabId)?.exited ?? false
  })
}

/** 聚焦某个 group 内应激活的窗格（焦点记忆 → 第一个有会话窗格）。 */
function focusGroupPane(group: TerminalGroup) {
  const tree = group.layout
  if (!tree) return
  const leaf =
    findLeaf(tree, group.focusedPaneId ?? '') ??
    firstLeafWithTab(tree) ??
    collectLeaves(tree)[0] ??
    null
  focusPane(leaf?.id ?? null)
}

/** 新建独立 Shell：新建一个 group，全屏单窗格（拆分布局只由拆分按钮产生）。 */
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
      const group = addGroup(groupsState.value, nextGroupId(), tab.id)
      focusGroupPane(group)
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

/** 拆分：在激活组的布局树上加一个窗格（新 session 属于同一 group，不产生新 tab）。 */
async function splitPane(direction: TerminalPaneDirection) {
  const group = activeGroup.value
  if (!group?.layout || !canSplit.value || opening) return
  opening = (async () => {
    loading.value = true
    error.value = ''
    try {
      const target =
        findLeaf(group.layout!, focusedPaneId.value ?? '') ?? firstLeafWithTab(group.layout!)
      if (!target) return
      const tab = await consoleStore.create(props.workspaceRoot, props.conversationId, 80, 24)
      const { node, newLeaf } = splitLeaf(group.layout!, target.id, direction)
      const next = setLeafTabId(node, newLeaf.id, tab.id)
      setGroupLayout(groupsState.value, group.id, next)
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

/** 关闭窗格：关 session 并从激活组布局移除；组只剩一个子节点时折叠；组空则删组切相邻。 */
async function closePane(leafId: string) {
  const group = activeGroup.value
  if (!group?.layout) return
  const { node, removed } = removeLeaf(group.layout, leafId)
  if (removed?.tabId) {
    try {
      await consoleStore.close(removed.tabId)
    } catch (cause) {
      showError(cause)
    }
  }
  if (!node) {
    // 组空了 → 删组并自动切到相邻组（优先下一个，其次上一个）
    const nextId = removeGroup(groupsState.value, group.id)
    focusedPaneId.value = null
    const next = nextId ? findGroup(groupsState.value, nextId) : null
    if (next) focusGroupPane(next)
    return
  }
  setGroupLayout(groupsState.value, group.id, node)
  const leaves = collectLeaves(node)
  const next = leaves.find(leaf => leaf.id === focusedPaneId.value) ?? leaves[0] ?? null
  focusPane(next?.id ?? null)
}

/** 关闭整个 group（tab bar X / 右键）：先删组切相邻，再逐个关 session。 */
async function closeTab(groupId: string) {
  error.value = ''
  const group = findGroup(groupsState.value, groupId)
  if (!group?.layout) return
  const sessionIds = collectLeaves(group.layout)
    .filter(leaf => leaf.tabId)
    .map(leaf => leaf.tabId as string)
  const nextId = removeGroup(groupsState.value, groupId)
  focusedPaneId.value = null
  for (const id of sessionIds) {
    try {
      await consoleStore.close(id)
    } catch (cause) {
      showError(cause)
    }
  }
  const next = nextId ? findGroup(groupsState.value, nextId) : null
  if (next) focusGroupPane(next)
}

/** tab bar 点击：切换 group，恢复该组布局与焦点。 */
function selectTab(groupId: string) {
  const from = activeGroup.value
  const ok = activateGroup(groupsState.value, groupId, from?.focusedPaneId ?? focusedPaneId.value)
  if (!ok) return
  const target = findGroup(groupsState.value, groupId)
  if (target) focusGroupPane(target)
}

async function restartActiveTab() {
  const leaf = activePaneLeaf.value
  const tab = leaf?.tabId ? workspaceTabs.value.find(item => item.id === leaf.tabId) : null
  const group = activeGroup.value
  if (!leaf || !tab || !group?.layout) return
  error.value = ''
  loading.value = true
  try {
    const fresh = await consoleStore.create(props.workspaceRoot, props.conversationId, 80, 24, tab.cwd)
    await consoleStore.close(tab.id)
    // 同步重新绑定：workspaceTabs 的失效清理 watch 之后才跑，此时 leaf.tabId
    // 已指向新会话，不会被当作失效窗格删除。
    const target = group.layout ? findLeaf(group.layout, leaf.id) : null
    if (target) {
      setGroupLayout(groupsState.value, group.id, setLeafTabId(group.layout, target.id, fresh.id))
      focusPane(target.id)
    } else {
      // 兜底：原窗格已不存在，作为新组全屏显示
      const freshGroup = addGroup(groupsState.value, nextGroupId(), fresh.id)
      focusGroupPane(freshGroup)
    }
  } catch (cause) {
    showError(cause)
  } finally {
    loading.value = false
  }
}

function openContextMenu(event: MouseEvent, group: TerminalGroup) {
  contextMenu.value = { group, x: event.clientX, y: event.clientY }
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

async function copyPath(group: TerminalGroup) {
  const tab = groupFocusedTab(group)
  if (tab) {
    try {
      await navigator.clipboard.writeText(tab.cwd)
    } catch (cause) {
      showError(cause)
    }
  }
  closeContextMenu()
}

/** 按当前 workspaceTabs 重建分组：每个现有 session 一个 group，激活 activeSessionId 对应组。 */
function rebuildGroupsFromTabs() {
  focusedPaneId.value = null
  const { state, activeGroup } = rebuildGroupsFromExistingTabs(
    workspaceTabs.value,
    consoleStore.activeSessionId,
    nextGroupId
  )
  groupsState.value = state
  if (activeGroup) focusGroupPane(activeGroup)
  return activeGroup
}

watch(contextMenu, menu => {
  if (menu) installOutsideMenuListeners()
  else removeOutsideMenuListeners?.()
})

watch(() => props.workspaceRoot, () => {
  const rebuilt = rebuildGroupsFromTabs()
  if (!rebuilt && props.workspaceRoot && props.active) void createTab()
})

watch(() => props.conversationId, () => {
  // 切换会话：按该会话现有 session 重建分组；没有则新建。
  const rebuilt = rebuildGroupsFromTabs()
  if (!rebuilt && props.workspaceRoot && props.active) void createTab()
})

// 会话被外部关闭（closeWorkspace / 其他入口）→ 从所有 group 移除失效窗格，
// 空组删除；激活组被删时切到相邻组。
watch(workspaceTabs, (tabs) => {
  const ids = new Set(tabs.map(tab => tab.id))
  for (const group of [...groupsState.value.groups]) {
    let tree = group.layout
    if (!tree) continue
    const deadLeaves = collectLeaves(tree).filter(
      leaf => leaf.tabId && !ids.has(leaf.tabId)
    )
    if (deadLeaves.length === 0) continue
    let current: TerminalPaneNode | null = tree
    for (const leaf of deadLeaves) {
      if (!current) break
      const found = findLeaf(current, leaf.id)
      if (!found) continue
      current = removeLeaf(current, found.id).node
    }
    if (!current) {
      // 整组失效 → 删组；若删的是激活组，removeGroup 已切相邻，补聚焦。
      const wasActive = groupsState.value.activeGroupId === group.id
      removeGroup(groupsState.value, group.id)
      if (wasActive) {
        focusedPaneId.value = null
        const nextId = groupsState.value.activeGroupId
        const next = nextId ? findGroup(groupsState.value, nextId) : null
        if (next) focusGroupPane(next)
      }
      continue
    }
    setGroupLayout(groupsState.value, group.id, current)
    // 激活组焦点窗格若被移除，回退聚焦。
    if (
      groupsState.value.activeGroupId === group.id &&
      !findLeaf(current, focusedPaneId.value ?? '')
    ) {
      const leaf = firstLeafWithTab(current) ?? collectLeaves(current)[0] ?? null
      focusPane(leaf?.id ?? null)
    }
  }
})

onMounted(() => {
  if (!props.active) return
  void (async () => {
    const rebuilt = rebuildGroupsFromTabs()
    if (!rebuilt) await createTab()
  })()
})

// KeepAlive 恢复：视图切回终端时组件不重新 mount（groups 保留），
// 但若 workspaceRoot 在隐藏期间变化导致 groups 被清空，需要重建。
onActivated(() => {
  if (!props.active) return
  if (groupsState.value.groups.length > 0) return
  const rebuilt = rebuildGroupsFromTabs()
  if (!rebuilt && props.workspaceRoot) void createTab()
})

onBeforeUnmount(() => {
  removeOutsideMenuListeners?.()
  removeOutsideMenuListeners = null
})
</script>

<template>
  <section class="terminal-panel relative flex min-h-0 flex-1 flex-col bg-transparent text-foreground" @click.self="closeContextMenu">
    <header class="console-chrome flex h-8 shrink-0 items-center gap-1 border-b border-border px-2 text-[11px]">
      <div class="console-tabs min-w-0 flex-1" role="tablist" :aria-label="t('workspace.terminalTabs')">
        <button
          v-for="(group, index) in groupsState.groups"
          :key="group.id"
          type="button"
          role="tab"
          class="console-tab"
          :class="groupsState.activeGroupId === group.id && 'is-active'"
          :aria-selected="groupsState.activeGroupId === group.id"
          :title="groupTitle(group)"
          @mousedown.left.prevent="selectTab(group.id)"
          @contextmenu.prevent="openContextMenu($event, group)"
        >
          <span class="truncate">{{ groupLabel(index) }}</span>
          <span v-if="groupHasExited(group)" class="text-warning">•</span>
          <X class="h-3 w-3 shrink-0 opacity-60 hover:opacity-100" @click.stop="closeTab(group.id)" />
        </button>
        <button
          v-if="hasWorkspace"
          type="button"
          class="console-new-tab"
          :title="t('workspace.newShell')"
          :aria-label="t('workspace.newShell')"
          :disabled="loading"
          @click="createTab()"
        ><FolderPlus class="h-3.5 w-3.5" /></button>
      </div>
      <span v-if="loading" class="flex items-center gap-1 text-muted"><Loader2 class="h-3 w-3 animate-spin" />{{ t('workspace.starting') }}</span>
      <button type="button" class="terminal-action" :title="t('workspace.splitHorizontal')" :aria-label="t('workspace.splitHorizontal')" :disabled="!canSplit" @click="splitPane('row')"><Columns2 class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" :title="t('workspace.splitVertical')" :aria-label="t('workspace.splitVertical')" :disabled="!canSplit" @click="splitPane('column')"><Rows2 class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" :title="t('workspace.restartShell')" :disabled="loading || !activePaneTab" @click="restartActiveTab"><RotateCcw class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" :title="t('workspace.killShell')" :disabled="loading || !activePaneTab" @click="activePaneLeaf && closePane(activePaneLeaf.id)"><X class="h-3.5 w-3.5" /></button>
    </header>
    <div
      v-if="contextMenu"
      class="console-context-menu"
      role="menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
    >
      <button type="button" role="menuitem" @click="copyPath(contextMenu.group)"><Copy class="h-3.5 w-3.5" />{{ t('workspace.copyFullPath') }}</button>
      <button type="button" role="menuitem" @click="closeTab(contextMenu.group.id); closeContextMenu()"><X class="h-3.5 w-3.5" />{{ t('workspace.closeShell') }}</button>
    </div>
    <p v-if="!hasWorkspace" class="m-auto max-w-56 text-center text-xs text-muted">{{ t('workspace.terminalNeedProject') }}</p>
    <div v-else-if="groupsState.groups.length === 0 && !loading" class="m-auto flex flex-col items-center gap-3 text-center text-xs text-muted">
      <p>{{ t('workspace.terminalEmptyHint') }}</p>
      <button type="button" class="console-create-first" @click="createTab()"><FolderPlus class="h-3.5 w-3.5" />{{ t('workspace.newTerminal') }}</button>
    </div>
    <p v-if="error" class="absolute inset-x-3 top-11 z-10 rounded border border-danger/40 bg-danger/10 p-2 text-xs text-danger">{{ error }}</p>
    <div
      v-show="hasWorkspace && groupsState.groups.length > 0"
      class="terminal-layout min-h-0 flex-1 p-2"
      data-workspace-terminal
    >
      <div
        v-for="group in groupsState.groups"
        v-show="group.id === groupsState.activeGroupId"
        :key="group.id"
        class="terminal-layout-group"
      >
        <TerminalSplitPane
          v-if="group.layout"
          :node="group.layout"
          :workspace-root="props.workspaceRoot"
          :conversation-id="props.conversationId"
          :focused-pane-id="group.id === groupsState.activeGroupId ? focusedPaneId : null"
          :visible="group.id === groupsState.activeGroupId"
          @focus="focusPane"
          @close="closePane"
        />
      </div>
    </div>
  </section>
</template>

<style scoped>
.terminal-layout { @apply relative flex min-h-0 flex-1; }
.terminal-layout-group { @apply absolute inset-0 flex min-h-0 min-w-0; }
.console-chrome,
.console-tabs,
.console-tab,
.console-new-tab,
.terminal-action { @apply select-none; }
.console-tabs { @apply flex min-w-0 items-center gap-1 overflow-x-auto; }
.console-tab { @apply flex max-w-32 items-center gap-1 rounded px-1.5 py-1 text-muted; }
.console-tab:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-tab.is-active { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-new-tab, .terminal-action { @apply shrink-0 rounded p-1 text-muted disabled:cursor-not-allowed disabled:opacity-40; }
.console-new-tab:hover, .terminal-action:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-context-menu { @apply fixed z-20 min-w-36 rounded border p-1 shadow-lg; border-color: hsl(var(--border)); background: hsl(var(--card)); transform: translateY(2px); }
.console-context-menu button { @apply flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground; }
.console-context-menu button:hover { background: hsl(var(--hover)); }
.console-create-first { @apply flex items-center gap-1 rounded border px-2 py-1.5 text-foreground; border-color: hsl(var(--border)); }
.console-create-first:hover { background: hsl(var(--hover)); }
</style>
