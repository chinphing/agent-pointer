<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Copy, ExternalLink, FileCode2, FileDiff, FolderOpen, GitBranch, Loader2, RefreshCw, X } from 'lucide-vue-next'
import {
  getTurnFileDiff,
  getWorkspaceGitDiff,
  getWorkspaceGitStatus,
  listWorkspaceDirectory,
  openPathWithDefaultApp,
  readWorkspaceFile,
  revealInFinder
} from '../../lib/api'
import type { DiffLine } from '../chat/DiffView.vue'
import type { GitChange, WorkspaceFilePreview as WorkspaceFilePreviewData } from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'
import { useWorkspacePanelStore } from '../../stores/workspacePanel'
import { workspaceRelativeDisplayPath } from '../../lib/toolCallDisplay'
import {
  clampContextMenuPosition,
  clampWorkspacePanelWidth,
  readWorkspacePanelWidth,
  resolveWorkspaceMarkdownReference,
  workspaceAbsolutePath
} from '../../lib/workspacePanel'
import { openExternalUrl } from '../../lib/openExternalUrl'
import {
  filterPreviewTabsForConversation,
  workspaceActiveAfterClose,
  workspacePreviewTabId,
  workspaceTabIdsToClose,
  type WorkspaceTabCloseAction
} from '../../lib/workspaceTabs'
import DiffView from '../chat/DiffView.vue'
import WorkspaceFilePreview from './WorkspaceFilePreview.vue'
import WorkspaceTreeNode from './WorkspaceTreeNode.vue'
import type { WorkspaceTreeNodeModel } from './WorkspaceTreeNode.vue'

const props = defineProps<{
  workspaceRoot: string
  /** Active conversation id — used to drop stale turn-diff tabs on switch. */
  conversationId?: string
}>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'initialize-git'): void
  (e: 'install-git'): void
}>()

type TreeNode = WorkspaceTreeNodeModel
type BasePreviewTab = {
  id: string
  path: string
  title: string
  loading: boolean
  error: string
  sizeBytes?: number
}
type FilePreviewTab = BasePreviewTab & {
  kind: 'file'
  preview: WorkspaceFilePreviewData | null
}
type DiffPreviewTab = BasePreviewTab & {
  kind: 'diff'
  change: GitChange
  mode: string
  diffLines: DiffLine[]
  diffStats: { adds: number; dels: number }
}
type TurnDiffPreviewTab = BasePreviewTab & {
  kind: 'turn-diff'
  conversationId: string
  turnId: string
  diffLines: DiffLine[]
  diffStats: { adds: number; dels: number }
  baselineMissing: boolean
}
type PreviewTab = FilePreviewTab | DiffPreviewTab | TurnDiffPreviewTab
type PrimaryView = 'files' | 'changes'
type ContextMenuState =
  | { kind: 'tree'; node: TreeNode; left: number; top: number }
  | { kind: 'change'; change: GitChange; left: number; top: number }
  | { kind: 'tab'; tabId: string; left: number; top: number }

const workspacePanelStore = useWorkspacePanelStore()
const WIDTH_STORAGE_KEY = 'pointer.workspacePanel.width'
const activeView = ref<PrimaryView | string>('files')
const previewTabs = ref<PreviewTab[]>([])
const roots = ref<TreeNode[]>([])
const changes = ref<GitChange[]>([])
const loadingFiles = ref(false)
const loadingChanges = ref(false)
const error = ref('')
const gitError = ref<import('../../lib/api').GitErrorInfo | null>(null)
const panelWidth = ref(readWorkspacePanelWidth(typeof localStorage === 'undefined' ? null : localStorage.getItem(WIDTH_STORAGE_KEY)))
const resizing = ref(false)
const contextMenu = ref<ContextMenuState | null>(null)
const isDesktop = isTauriRuntime()
let resizeStartX = 0
let resizeStartWidth = 0

const IMAGE_EXTS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'])

function isMediaFile(path: string): 'image' | 'pdf' | null {
  const ext = path.split('.').pop()?.toLowerCase() || ''
  if (IMAGE_EXTS.has(ext)) return 'image'
  if (ext === 'pdf') return 'pdf'
  return null
}

const hasWorkspace = computed(() => !!props.workspaceRoot.trim())
const workspaceName = computed(() => props.workspaceRoot.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || props.workspaceRoot)
const panelStyle = computed(() => ({ width: `${panelWidth.value}px` }))
const activePreviewTab = computed(() => previewTabs.value.find(item => item.id === activeView.value) ?? null)
const activeFileTab = computed(() => activePreviewTab.value?.kind === 'file' ? activePreviewTab.value : null)
const activeDiffTab = computed(() => activePreviewTab.value?.kind === 'diff' ? activePreviewTab.value : null)
const activeTurnDiffTab = computed(() => activePreviewTab.value?.kind === 'turn-diff' ? activePreviewTab.value : null)

/** Row kept highlighted while its context menu is open (hover alone disappears under the overlay). */
const contextSelectedTreePath = computed(() =>
  contextMenu.value?.kind === 'tree' ? contextMenu.value.node.path : ''
)
const contextSelectedChangeKey = computed(() => {
  if (contextMenu.value?.kind !== 'change') return ''
  const change = contextMenu.value.change
  return `${change.staged ? '1' : '0'}:${change.path}`
})
const contextSelectedTabId = computed(() =>
  contextMenu.value?.kind === 'tab' ? contextMenu.value.tabId : ''
)

function changeRowKey(change: GitChange): string {
  return `${change.staged ? '1' : '0'}:${change.path}`
}

/** Drop stale async results when the user switches workspace / tabs quickly. */
let rootLoadSeq = 0
let changesLoadSeq = 0

/**
 * Load the file tree.
 * `silent` keeps the previous tree visible (stale-while-revalidate) so clicks stay snappy.
 */
async function loadRoot(options?: { silent?: boolean }) {
  const silent = options?.silent === true
  const seq = ++rootLoadSeq
  const workspaceRoot = props.workspaceRoot
  if (!hasWorkspace.value) {
    roots.value = []
    return
  }
  if (!silent) {
    roots.value = []
    loadingFiles.value = true
    error.value = ''
  }
  try {
    const entries = (await listWorkspaceDirectory(workspaceRoot)).map(entry => ({ ...entry }))
    if (seq !== rootLoadSeq || props.workspaceRoot !== workspaceRoot) return
    roots.value = entries
  } catch (err) {
    if (seq !== rootLoadSeq || props.workspaceRoot !== workspaceRoot) return
    const message = err instanceof Error ? err.message : String(err)
    if (silent) {
      console.warn('[WorkspacePanel] background file tree failed', message)
    } else {
      error.value = message
      roots.value = []
    }
  } finally {
    if (!silent && seq === rootLoadSeq) loadingFiles.value = false
  }
}

async function toggleDirectory(node: TreeNode) {
  if (node.kind !== 'directory') return
  node.expanded = !node.expanded
  if (!node.expanded || node.children) return
  node.loading = true
  error.value = ''
  try {
    node.children = (await listWorkspaceDirectory(props.workspaceRoot, node.path)).map(entry => ({ ...entry }))
  } catch (err) {
    node.expanded = false
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    node.loading = false
  }
}

/**
 * Load git status for the Changes tab and the header badge.
 * `silent` keeps the previous badge count visible and avoids flipping the
 * Changes-tab loading UI (used for background refresh while on Files).
 */
async function loadChanges(options?: { silent?: boolean }) {
  const silent = options?.silent === true
  const seq = ++changesLoadSeq
  const workspaceRoot = props.workspaceRoot
  if (!hasWorkspace.value) {
    changes.value = []
    gitError.value = null
    return
  }
  if (!silent) {
    changes.value = []
    loadingChanges.value = true
    error.value = ''
    gitError.value = null
  }
  try {
    const result = await getWorkspaceGitStatus(workspaceRoot)
    if (seq !== changesLoadSeq || props.workspaceRoot !== workspaceRoot) return
    changes.value = result.changes
    gitError.value = result.error ?? null
  } catch (err) {
    if (seq !== changesLoadSeq || props.workspaceRoot !== workspaceRoot) return
    const message = err instanceof Error ? err.message : String(err)
    if (silent) {
      console.warn('[WorkspacePanel] background git status failed', message)
    } else {
      changes.value = []
      error.value = message
    }
  } finally {
    if (!silent && seq === changesLoadSeq) loadingChanges.value = false
  }
}

/** Keep the Changes badge current even when the Files tab is active. */
function refreshChangesBadge() {
  void loadChanges({ silent: true })
}

/** Switch primary nav immediately; refresh in the background. */
function activatePrimaryView(nextView: PrimaryView) {
  activeView.value = nextView
  if (nextView === 'files') {
    void loadRoot({ silent: true })
    refreshChangesBadge()
  } else {
    void loadChanges({ silent: true })
  }
}

function reloadPreviewTab(tabItem: PreviewTab) {
  if (tabItem.kind === 'file') void loadFileTab(tabItem)
  else if (tabItem.kind === 'turn-diff') void loadTurnDiffTab(tabItem)
  else void loadDiffTab(tabItem)
}

async function loadFileTab(tabItem: FilePreviewTab) {
  tabItem.loading = true
  tabItem.error = ''
  try {
    const mediaKind = isMediaFile(tabItem.path)
    if (mediaKind && isDesktop) {
      const absPath = workspaceAbsolutePath(props.workspaceRoot, tabItem.path)
      tabItem.preview = { path: absPath, sizeBytes: tabItem.sizeBytes ?? 0, truncated: false, binary: false }
      tabItem.loading = false
      return
    }
    tabItem.preview = await readWorkspaceFile(props.workspaceRoot, tabItem.path)
  } catch (err) {
    tabItem.preview = null
    tabItem.error = err instanceof Error ? err.message : String(err)
  } finally {
    tabItem.loading = false
  }
}

async function loadDiffTab(tabItem: DiffPreviewTab) {
  tabItem.loading = true
  tabItem.error = ''
  try {
    const result = await getWorkspaceGitDiff(
      props.workspaceRoot,
      tabItem.path,
      tabItem.change.status
    )
    tabItem.mode = result.mode ?? ''
    tabItem.diffLines = (result.diffLines ?? []) as DiffLine[]
    tabItem.diffStats = {
      adds: result.diffStats?.adds ?? 0,
      dels: result.diffStats?.dels ?? 0
    }
  } catch (err) {
    tabItem.diffLines = []
    tabItem.diffStats = { adds: 0, dels: 0 }
    tabItem.error = err instanceof Error ? err.message : String(err)
  } finally {
    tabItem.loading = false
  }
}

async function loadTurnDiffTab(tabItem: TurnDiffPreviewTab) {
  tabItem.loading = true
  tabItem.error = ''
  try {
    const result = await getTurnFileDiff(
      tabItem.conversationId,
      tabItem.turnId,
      props.workspaceRoot,
      tabItem.path
    )
    tabItem.diffLines = (result.diffLines ?? []) as DiffLine[]
    tabItem.diffStats = {
      adds: result.diffStats?.adds ?? 0,
      dels: result.diffStats?.dels ?? 0
    }
    tabItem.baselineMissing = Boolean(result.baselineMissing)
    if (result.baselineMissing) {
      tabItem.error = '未找到本轮修改前快照，显示结果可能不完整'
    }
  } catch (err) {
    tabItem.diffLines = []
    tabItem.diffStats = { adds: 0, dels: 0 }
    tabItem.error = err instanceof Error ? err.message : String(err)
  } finally {
    tabItem.loading = false
  }
}

function openTurnDiff(conversationId: string, turnId: string, path: string) {
  if (!props.workspaceRoot.trim()) {
    error.value = '请先选择工作区'
    return
  }
  const id = workspacePreviewTabId('turn-diff', path, turnId)
  const existing = previewTabs.value.find(item => item.id === id)
  if (existing) {
    activeView.value = id
    if (existing.kind === 'turn-diff') void loadTurnDiffTab(existing)
    refreshChangesBadge()
    return
  }
  const relative = workspaceRelativeDisplayPath(path, props.workspaceRoot)
  const tabItem: TurnDiffPreviewTab = {
    id,
    kind: 'turn-diff',
    path,
    title: relative.split(/[\\/]/).pop() || relative,
    loading: false,
    error: '',
    conversationId,
    turnId,
    diffLines: [],
    diffStats: { adds: 0, dels: 0 },
    baselineMissing: false
  }
  previewTabs.value.push(tabItem)
  const reactiveTab = previewTabs.value[previewTabs.value.length - 1] as TurnDiffPreviewTab
  activeView.value = id
  void loadTurnDiffTab(reactiveTab)
  refreshChangesBadge()
}

/** Explicit toolbar refresh — shows loading; still non-blocking for the click handler. */
function refreshActiveTab() {
  if (activeView.value === 'files') {
    void loadRoot()
    refreshChangesBadge()
  } else if (activeView.value === 'changes') {
    void loadChanges()
  } else if (activeFileTab.value) {
    void loadFileTab(activeFileTab.value)
    refreshChangesBadge()
  } else if (activeDiffTab.value) {
    void loadDiffTab(activeDiffTab.value)
    refreshChangesBadge()
  } else if (activeTurnDiffTab.value) {
    void loadTurnDiffTab(activeTurnDiffTab.value)
    refreshChangesBadge()
  }
}

/** Conversation / workspace switch: keep current UI, refresh quietly in background. */
function refreshActiveTabInBackground() {
  if (activeView.value === 'files') {
    void loadRoot({ silent: true })
    refreshChangesBadge()
  } else if (activeView.value === 'changes') {
    void loadChanges({ silent: true })
  } else if (activeFileTab.value) {
    void loadFileTab(activeFileTab.value)
    refreshChangesBadge()
  } else if (activeDiffTab.value) {
    void loadDiffTab(activeDiffTab.value)
    refreshChangesBadge()
  } else if (activeTurnDiffTab.value) {
    void loadTurnDiffTab(activeTurnDiffTab.value)
    refreshChangesBadge()
  }
}

function selectFile(node: Pick<TreeNode, 'kind' | 'name' | 'path'> & { sizeBytes?: number }) {
  if (node.kind !== 'file') return
  const id = workspacePreviewTabId('file', node.path)
  const existing = previewTabs.value.find(item => item.id === id)
  if (existing) {
    activeView.value = id
    if (existing.kind === 'file') void loadFileTab(existing)
    return
  }
  const tabItem: FilePreviewTab = {
    id,
    kind: 'file',
    path: node.path,
    title: node.name,
    loading: false,
    error: '',
    preview: null,
    sizeBytes: node.sizeBytes
  }
  previewTabs.value.push(tabItem)
  const reactiveTab = previewTabs.value[previewTabs.value.length - 1] as FilePreviewTab
  activeView.value = id
  void loadFileTab(reactiveTab)
}

async function openWorkspaceReference(path: string) {
  const id = workspacePreviewTabId('file', path)
  const existing = previewTabs.value.find(item => item.id === id)
  if (existing) {
    activeView.value = id
    if (existing.kind === 'file') void loadFileTab(existing)
    return
  }

  try {
    // Probe first so missing Markdown targets do not leave an empty tab.
    const preview = await readWorkspaceFile(props.workspaceRoot, path)
    const mediaKind = isMediaFile(path)
    const tabItem: FilePreviewTab = {
      id,
      kind: 'file',
      path,
      title: path.split('/').pop() || path,
      loading: false,
      error: '',
      preview: mediaKind && isDesktop
        ? {
            path: workspaceAbsolutePath(props.workspaceRoot, path),
            sizeBytes: preview.sizeBytes,
            truncated: false,
            binary: false
          }
        : preview,
      sizeBytes: preview.sizeBytes
    }
    previewTabs.value.push(tabItem)
    activeView.value = id
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.warn('[WorkspacePanel] Markdown reference target is unavailable', { path, message })
  }
}

async function openMarkdownReference(href: string) {
  const sourcePath = activeFileTab.value?.path
  if (!sourcePath) {
    console.warn('[WorkspacePanel] Markdown reference has no active source file', href)
    return
  }

  const reference = resolveWorkspaceMarkdownReference(props.workspaceRoot, sourcePath, href)
  try {
    error.value = ''
    if (reference.kind === 'external') {
      await openExternalUrl(reference.url)
      return
    }
    if (reference.kind === 'workspace') {
      await openWorkspaceReference(reference.path)
      return
    }
    if (reference.kind === 'local') {
      if (!isDesktop) throw new Error('网页端无法打开工作区外的本地文件')
      await openPathWithDefaultApp(reference.path)
      return
    }
    throw new Error(`不支持打开此链接：${reference.label}`)
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.warn('[WorkspacePanel] Failed to open Markdown reference', { href, message })
  }
}

function selectChange(change: GitChange) {
  const id = workspacePreviewTabId('diff', change.path)
  const existing = previewTabs.value.find(item => item.id === id)
  if (existing) {
    activeView.value = id
    if (existing.kind === 'diff') void loadDiffTab(existing)
    return
  }
  const tabItem: DiffPreviewTab = {
    id,
    kind: 'diff',
    path: change.path,
    title: change.path.split('/').pop() || change.path,
    loading: false,
    error: '',
    change,
    mode: '',
    diffLines: [],
    diffStats: { adds: 0, dels: 0 }
  }
  previewTabs.value.push(tabItem)
  const reactiveTab = previewTabs.value[previewTabs.value.length - 1] as DiffPreviewTab
  activeView.value = id
  void loadDiffTab(reactiveTab)
}

function activatePreviewTab(tabId: string) {
  activeView.value = tabId
  const tabItem = previewTabs.value.find(item => item.id === tabId)
  if (tabItem) reloadPreviewTab(tabItem)
}

function closePreviewTabs(targetId: string, action: WorkspaceTabCloseAction = 'close') {
  const ids = previewTabs.value.map(item => item.id)
  const closingIds = workspaceTabIdsToClose(ids, targetId, action)
  const target = previewTabs.value.find(item => item.id === targetId)
  const fallback: PrimaryView = target?.kind === 'diff' || target?.kind === 'turn-diff' ? 'changes' : 'files'
  activeView.value = workspaceActiveAfterClose(ids, activeView.value, closingIds, targetId, fallback)
  previewTabs.value = previewTabs.value.filter(item => !closingIds.includes(item.id))
}

function statusLabel(change: GitChange) {
  return `${change.staged ? 'S' : ''}${change.status}`
}

function beginResize(event: MouseEvent) {
  if (event.button !== 0) return
  event.preventDefault()
  resizing.value = true
  resizeStartX = event.clientX
  resizeStartWidth = panelWidth.value
  window.addEventListener('mousemove', resizePanel)
  window.addEventListener('mouseup', finishResize)
}

function resizePanel(event: MouseEvent) {
  panelWidth.value = clampWorkspacePanelWidth(resizeStartWidth + resizeStartX - event.clientX)
}

function finishResize() {
  if (!resizing.value) return
  resizing.value = false
  localStorage.setItem(WIDTH_STORAGE_KEY, String(panelWidth.value))
  window.removeEventListener('mousemove', resizePanel)
  window.removeEventListener('mouseup', finishResize)
}

function handleViewportResize() {
  panelWidth.value = clampWorkspacePanelWidth(panelWidth.value)
}

function menuPosition(event: MouseEvent, menuHeight: number) {
  window.getSelection()?.removeAllRanges()
  return clampContextMenuPosition(event.clientX, event.clientY, window.innerWidth, window.innerHeight, 208, menuHeight)
}

function openTreeContextMenu(event: MouseEvent, node: TreeNode) {
  contextMenu.value = { kind: 'tree', node, ...menuPosition(event, isDesktop ? 246 : 190) }
}

function openChangeContextMenu(event: MouseEvent, change: GitChange) {
  contextMenu.value = { kind: 'change', change, ...menuPosition(event, isDesktop ? 206 : 174) }
}

function openTabContextMenu(event: MouseEvent, tabId: string) {
  contextMenu.value = { kind: 'tab', tabId, ...menuPosition(event, isDesktop ? 282 : 250) }
}

function closeContextMenu() {
  contextMenu.value = null
}

async function runPathAction(path: string, action: 'copy-absolute' | 'copy-relative' | 'reveal') {
  const absolutePath = workspaceAbsolutePath(props.workspaceRoot, path)
  if (action === 'copy-absolute') await navigator.clipboard.writeText(absolutePath)
  else if (action === 'copy-relative') await navigator.clipboard.writeText(path)
  else await revealInFinder(absolutePath)
}

async function runTreeContextAction(action: 'preview' | 'open-system' | 'copy-absolute' | 'copy-relative' | 'reveal' | 'refresh') {
  const menu = contextMenu.value
  closeContextMenu()
  if (menu?.kind !== 'tree') return
  const node = menu.node
  try {
    if (action === 'preview') {
      if (node.kind === 'directory') await toggleDirectory(node)
      else await selectFile(node)
    } else if (action === 'open-system') {
      await openPathWithDefaultApp(workspaceAbsolutePath(props.workspaceRoot, node.path))
    } else if (action === 'refresh') await loadRoot()
    else await runPathAction(node.path, action)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

async function runChangeContextAction(action: 'diff' | 'preview' | 'copy-absolute' | 'copy-relative' | 'reveal') {
  const menu = contextMenu.value
  closeContextMenu()
  if (menu?.kind !== 'change') return
  try {
    if (action === 'diff') await selectChange(menu.change)
    else if (action === 'preview') {
      await selectFile({ kind: 'file', name: menu.change.path.split('/').pop() || menu.change.path, path: menu.change.path })
    } else await runPathAction(menu.change.path, action)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

async function runTabContextAction(action: WorkspaceTabCloseAction | 'copy-absolute' | 'copy-relative' | 'reveal' | 'refresh') {
  const menu = contextMenu.value
  closeContextMenu()
  if (menu?.kind !== 'tab') return
  const tabItem = previewTabs.value.find(item => item.id === menu.tabId)
  if (!tabItem) return
  try {
    if (action === 'close' || action === 'close-others' || action === 'close-right') {
      closePreviewTabs(tabItem.id, action)
    } else if (action === 'refresh') {
      await reloadPreviewTab(tabItem)
    } else await runPathAction(tabItem.path, action)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeContextMenu()
}

const normalizedWorkspaceRoot = computed(() => props.workspaceRoot.trim().replace(/[\\/]+$/, ''))
const normalizedConversationId = computed(() => props.conversationId?.trim() ?? '')
let loadedWorkspaceRoot = ''
let loadedConversationId = ''

function resetWorkspaceSurface(nextRoot: string, nextConversationId: string) {
  loadedWorkspaceRoot = nextRoot
  loadedConversationId = nextConversationId
  previewTabs.value = []
  roots.value = []
  changes.value = []
  gitError.value = null
  error.value = ''
  activeView.value = 'files'
  closeContextMenu()
  // Invalidate in-flight loads from the previous workspace.
  rootLoadSeq++
  changesLoadSeq++
  void loadRoot({ silent: true })
  refreshChangesBadge()
}

function onConversationChanged(nextConversationId: string) {
  loadedConversationId = nextConversationId
  // Turn-diff tabs are conversation-scoped; keep file/git tabs for the same workspace.
  previewTabs.value = filterPreviewTabsForConversation(previewTabs.value, nextConversationId)
  if (
    activeView.value !== 'files'
    && activeView.value !== 'changes'
    && !previewTabs.value.some(tab => tab.id === activeView.value)
  ) {
    activeView.value = 'files'
  }
  closeContextMenu()
  refreshActiveTabInBackground()
}

watch(
  [normalizedWorkspaceRoot, normalizedConversationId],
  ([nextRoot, nextConversationId], previous) => {
    const previousRoot = previous?.[0] ?? ''
    const previousConversationId = previous?.[1] ?? ''
    // Conversation hydration can briefly expose an empty root while refreshing.
    // Do not destroy the user's open tabs and diff state for that transient value.
    if (!nextRoot && previousRoot && loadedWorkspaceRoot) return

    const rootChanged = nextRoot !== loadedWorkspaceRoot
    if (rootChanged) {
      resetWorkspaceSurface(nextRoot, nextConversationId)
      return
    }

    // Same workspace: ignore a transient empty conversation id during switch.
    if (!nextConversationId && previousConversationId && loadedConversationId) return
    if (nextConversationId === loadedConversationId) return
    onConversationChanged(nextConversationId)
  },
  { immediate: true }
)

watch(
  () => workspacePanelStore.pendingTurnDiff,
  (request) => {
    if (!request) return
    const pending = workspacePanelStore.consumePendingTurnDiff()
    if (!pending) return
    void openTurnDiff(pending.conversationId, pending.turnId, pending.path)
  }
)

onMounted(() => {
  window.addEventListener('resize', handleViewportResize)
  window.addEventListener('scroll', closeContextMenu, true)
  document.addEventListener('keydown', handleDocumentKeydown)
  const pending = workspacePanelStore.consumePendingTurnDiff()
  if (pending) void openTurnDiff(pending.conversationId, pending.turnId, pending.path)
})

onBeforeUnmount(() => {
  finishResize()
  window.removeEventListener('resize', handleViewportResize)
  window.removeEventListener('scroll', closeContextMenu, true)
  document.removeEventListener('keydown', handleDocumentKeydown)
})
</script>

<template>
  <aside
    class="workspace-panel hidden lg:flex shrink-0 flex-col min-h-0 border-l border-border bg-card relative"
    data-workspace-panel
    :class="resizing && 'is-resizing'"
    :style="panelStyle"
  >
    <div class="workspace-resize-handle" title="拖动调整宽度" @mousedown="beginResize" />

    <header class="h-10 shrink-0 flex items-center gap-2 px-3 border-b border-border">
      <FolderOpen class="w-4 h-4 text-accent" />
      <span class="text-xs font-semibold truncate flex-1" :title="workspaceRoot">{{ workspaceName || '工作区' }}</span>
      <button class="chrome-icon-btn" title="关闭工作区" type="button" @click="$emit('close')"><X class="w-4 h-4" /></button>
    </header>

    <div class="workspace-tabs-bar">
      <button
        type="button"
        class="workspace-tab workspace-tab-icon shrink-0"
        :class="activeView === 'files' && 'is-active'"
        title="工作区文件"
        aria-label="工作区文件"
        @click="activatePrimaryView('files')"
      >
        <FolderOpen class="w-3.5 h-3.5" />
      </button>
      <button
        type="button"
        class="workspace-tab workspace-tab-icon shrink-0"
        :class="activeView === 'changes' && 'is-active'"
        title="变更文件"
        aria-label="变更文件"
        @click="activatePrimaryView('changes')"
      >
        <GitBranch class="w-3.5 h-3.5" />
        <span v-if="changes.length" class="workspace-tab-badge">{{ changes.length }}</span>
      </button>
      <div class="workspace-preview-tabs">
        <button
          v-for="previewTab in previewTabs"
          :key="previewTab.id"
          type="button"
          class="workspace-tab workspace-preview-tab"
          :class="[
            activeView === previewTab.id && 'is-active',
            contextSelectedTabId === previewTab.id && 'is-context-selected'
          ]"
          :title="previewTab.path"
          @click="activatePreviewTab(previewTab.id)"
          @contextmenu.prevent="openTabContextMenu($event, previewTab.id)"
        >
          <component :is="previewTab.kind === 'file' ? FileCode2 : FileDiff" class="w-3 h-3 shrink-0" />
          <span class="truncate max-w-24">{{ previewTab.title }}</span>
          <X class="w-3 h-3 shrink-0" @click.stop="closePreviewTabs(previewTab.id)" />
        </button>
      </div>
      <button type="button" class="ml-auto px-3 text-muted hover:text-foreground shrink-0" title="刷新" @click="refreshActiveTab"><RefreshCw class="w-3.5 h-3.5" /></button>
    </div>

    <div v-if="!hasWorkspace" class="flex-1 grid place-items-center p-6 text-center text-xs text-muted">
      请先在输入区选择项目目录，工作区文件与 Git 变更会显示在这里。
    </div>
    <div v-else-if="error" class="p-4 text-xs text-danger break-words">{{ error }}</div>

    <div v-else-if="activeView === 'files'" class="flex-1 min-h-0 overflow-auto py-2">
      <div v-if="loadingFiles" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载中…</div>
      <div v-else-if="!roots.length" class="workspace-empty">目录为空</div>
      <WorkspaceTreeNode
        v-for="node in roots"
        :key="node.path"
        :node="node"
        :highlighted-path="contextSelectedTreePath"
        @toggle="toggleDirectory"
        @activate="selectFile"
        @contextmenu="openTreeContextMenu"
      />
    </div>

    <div v-else-if="activeFileTab" class="flex-1 min-h-0 overflow-hidden p-2">
      <div v-if="activeFileTab.loading" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载文件…</div>
      <div v-else-if="activeFileTab.error" class="text-xs text-danger break-words p-2">{{ activeFileTab.error }}</div>
      <WorkspaceFilePreview
        v-else-if="activeFileTab.preview"
        :preview="activeFileTab.preview"
        :absolute-path="workspaceAbsolutePath(workspaceRoot, activeFileTab.path)"
        @open-reference="openMarkdownReference"
      />
      <div v-else class="workspace-empty">无法显示该文件</div>
    </div>

    <div v-else-if="activeView === 'changes'" class="flex-1 min-h-0 overflow-auto py-2">
      <div v-if="loadingChanges" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载中…</div>
      <div v-else-if="gitError" class="workspace-empty flex-col text-center">
        <GitBranch class="w-4 h-4" />
        <span>{{ gitError.code === 'not_repository' ? '当前目录不是 Git 仓库' : gitError.code === 'git_not_installed' ? '未检测到 Git' : gitError.message }}</span>
        <button
          v-if="gitError.code === 'not_repository'"
          type="button"
          class="workspace-action-btn"
          @click="emit('initialize-git')"
        >让 Pointer 初始化 Git 仓库</button>
        <button
          v-else-if="gitError.code === 'git_not_installed'"
          type="button"
          class="workspace-action-btn"
          @click="emit('install-git')"
        >帮我安装 Git</button>
      </div>
      <div v-else-if="!changes.length" class="workspace-empty"><GitBranch class="w-4 h-4" /> 没有 Git 变更</div>
      <button
        v-for="change in changes"
        :key="`${change.staged}-${change.path}`"
        type="button"
        class="change-row"
        :class="contextSelectedChangeKey === changeRowKey(change) && 'is-selected'"
        @click="selectChange(change)"
        @contextmenu.prevent="openChangeContextMenu($event, change)"
      >
        <span class="status-badge">{{ statusLabel(change) }}</span><span class="truncate select-none" :title="change.path">{{ change.path }}</span>
      </button>
    </div>

    <div v-else-if="activeTurnDiffTab" class="flex-1 min-h-0 overflow-hidden p-2">
      <div v-if="activeTurnDiffTab.loading" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载 Diff…</div>
      <div v-else-if="activeTurnDiffTab.error && !activeTurnDiffTab.diffLines.length" class="text-xs text-danger break-words p-2">{{ activeTurnDiffTab.error }}</div>
      <template v-else>
        <p v-if="activeTurnDiffTab.error" class="text-[11px] text-muted px-1 pb-1">{{ activeTurnDiffTab.error }}</p>
        <DiffView
          v-if="activeTurnDiffTab.diffLines.length"
          fill-height
          :diff-lines="activeTurnDiffTab.diffLines"
          :diff-stats="activeTurnDiffTab.diffStats"
        />
        <div v-else class="workspace-empty">该文件没有可显示的文本 Diff</div>
      </template>
    </div>

    <div v-else-if="activeDiffTab" class="flex-1 min-h-0 overflow-hidden p-2">
      <div v-if="activeDiffTab.loading" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载 Diff…</div>
      <div v-else-if="activeDiffTab.error" class="text-xs text-danger break-words p-2">{{ activeDiffTab.error }}</div>
      <DiffView
        v-else-if="activeDiffTab.diffLines.length"
        fill-height
        :diff-lines="activeDiffTab.diffLines"
        :diff-stats="activeDiffTab.diffStats"
      />
      <div v-else class="workspace-empty">该文件没有可显示的文本 Diff</div>
    </div>
    <div v-else class="workspace-empty">预览标签已关闭</div>

    <Teleport to="body">
      <div v-if="contextMenu" class="fixed inset-0 z-[300]" @mousedown="closeContextMenu" @contextmenu.prevent="closeContextMenu">
        <div
          class="workspace-context-menu"
          role="menu"
          :style="{ left: `${contextMenu.left}px`, top: `${contextMenu.top}px` }"
          @mousedown.stop
        >
          <template v-if="contextMenu.kind === 'tree'">
            <button type="button" role="menuitem" @click="runTreeContextAction('preview')"><FileCode2 />{{ contextMenu.node.kind === 'directory' ? '展开/收起' : '预览文件' }}</button>
            <button v-if="isDesktop && contextMenu.node.kind === 'file'" type="button" role="menuitem" @click="runTreeContextAction('open-system')"><ExternalLink />使用默认应用打开</button>
            <div class="workspace-context-separator" />
            <button type="button" role="menuitem" @click="runTreeContextAction('copy-absolute')"><Copy />复制绝对路径</button>
            <button type="button" role="menuitem" @click="runTreeContextAction('copy-relative')"><Copy />复制相对路径</button>
            <button v-if="isDesktop" type="button" role="menuitem" @click="runTreeContextAction('reveal')"><FolderOpen />在 Finder 中显示</button>
            <button type="button" role="menuitem" @click="runTreeContextAction('refresh')"><RefreshCw />刷新文件树</button>
          </template>

          <template v-else-if="contextMenu.kind === 'change'">
            <button type="button" role="menuitem" @click="runChangeContextAction('diff')"><FileDiff />查看 Diff</button>
            <button type="button" role="menuitem" @click="runChangeContextAction('preview')"><FileCode2 />预览文件</button>
            <div class="workspace-context-separator" />
            <button type="button" role="menuitem" @click="runChangeContextAction('copy-absolute')"><Copy />复制绝对路径</button>
            <button type="button" role="menuitem" @click="runChangeContextAction('copy-relative')"><Copy />复制相对路径</button>
            <button v-if="isDesktop" type="button" role="menuitem" @click="runChangeContextAction('reveal')"><FolderOpen />在 Finder 中显示</button>
          </template>

          <template v-else>
            <button type="button" role="menuitem" @click="runTabContextAction('close')"><X />关闭</button>
            <button type="button" role="menuitem" @click="runTabContextAction('close-others')"><X />关闭其他标签</button>
            <button type="button" role="menuitem" @click="runTabContextAction('close-right')"><X />关闭右侧标签</button>
            <button type="button" role="menuitem" @click="runTabContextAction('refresh')"><RefreshCw />刷新标签</button>
            <div class="workspace-context-separator" />
            <button type="button" role="menuitem" @click="runTabContextAction('copy-absolute')"><Copy />复制绝对路径</button>
            <button type="button" role="menuitem" @click="runTabContextAction('copy-relative')"><Copy />复制相对路径</button>
            <button v-if="isDesktop" type="button" role="menuitem" @click="runTabContextAction('reveal')"><FolderOpen />在 Finder 中显示</button>
          </template>
        </div>
      </div>
    </Teleport>
  </aside>
</template>

<style scoped>
.workspace-panel.is-resizing { @apply select-none; }
.workspace-tabs-bar { @apply flex shrink-0 min-w-0 border-b border-border; }
.workspace-preview-tabs { @apply flex min-w-0 flex-1 overflow-x-auto; scrollbar-width: thin; }
.workspace-preview-tab { @apply min-w-0 shrink-0 flex items-center gap-1; }
.workspace-resize-handle { @apply absolute inset-y-0 -left-1 w-2 cursor-col-resize z-20; }
.workspace-resize-handle:hover::after, .workspace-panel.is-resizing .workspace-resize-handle::after { content: ''; @apply absolute inset-y-0 left-1 w-px bg-accent; }
.workspace-tab { @apply px-3 py-2 text-xs text-muted border-b-2 border-transparent; }
.workspace-tab.is-active { @apply text-foreground border-accent; }
.workspace-tab.is-context-selected { @apply bg-accent/10 text-foreground; }
.workspace-tab-icon {
  @apply relative inline-flex items-center justify-center px-2.5;
}
.workspace-tab-badge {
  @apply absolute -right-0.5 -top-0.5 min-w-3.5 rounded-full bg-accent px-1 text-[9px] leading-3 text-white;
}
.workspace-empty { @apply p-4 text-xs text-muted flex items-center justify-center gap-2; }
.workspace-action-btn { @apply rounded border border-border px-2.5 py-1.5 text-xs text-foreground hover:bg-hover disabled:opacity-50; }
.change-row { @apply w-full flex items-center gap-1.5 px-3 py-1.5 text-left text-xs hover:bg-hover; }
.change-row.is-selected { @apply bg-accent/10 text-foreground; }
.status-badge { @apply min-w-7 text-center font-mono text-[10px] text-accent; }
.workspace-context-menu { @apply fixed z-[301] w-52 rounded-md border border-border bg-card p-1 shadow-xl select-none; }
.workspace-context-separator { @apply my-1 border-t border-border; }
.workspace-context-menu button { @apply w-full flex items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground hover:bg-hover; }
.workspace-context-menu button :deep(svg) { @apply w-3.5 h-3.5 text-muted; }
</style>
