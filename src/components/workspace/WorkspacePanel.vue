<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import {
  ArrowDown,
  ArrowUp,
  Copy,
  ExternalLink,
  FileCode2,
  FileDiff,
  FolderOpen,
  GitBranch,
  Loader2,
  RefreshCw,
  Search,
  SquareTerminal,
  Trash2,
  X
} from 'lucide-vue-next'
import {
  deleteWorkspacePath,
  getTurnFileDiff,
  getWorkspaceGitDiff,
  getWorkspaceGitStatus,
  listWorkspaceDirectory,
  openPathWithDefaultApp,
  readWorkspaceFile,
  revealInFinder,
  searchWorkspaceEntries
} from '../../lib/api'
import type { DiffLine } from '../chat/DiffView.vue'
import type { GitChange, WorkspaceEntry, WorkspaceFilePreview as WorkspaceFilePreviewData } from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'
import { lastTurnFileChanges } from '../../lib/lastTurnFileChanges'
import { useChatStore } from '../../stores/chat'
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
import {
  findWorkspaceTreeNode,
  mergeWorkspaceTreePreserveState,
  workspacePathAncestorDirs
} from '../../lib/workspaceTree'
import DiffView from '../chat/DiffView.vue'
import WorkspaceFilePreview from './WorkspaceFilePreview.vue'
import WorkspaceTreeNode from './WorkspaceTreeNode.vue'
import type { WorkspaceTreeNodeModel } from './WorkspaceTreeNode.vue'

const TerminalPanel = defineAsyncComponent(() => import('./TerminalPanel.vue'))

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
type PrimaryView = 'terminal' | 'files' | 'changes'
type ContextMenuState =
  | { kind: 'tree'; node: TreeNode; left: number; top: number }
  | { kind: 'change'; change: GitChange; left: number; top: number }
  | { kind: 'tab'; tabId: string; left: number; top: number }

const workspacePanelStore = useWorkspacePanelStore()
const chat = useChatStore()
const WIDTH_STORAGE_KEY = 'pointer.workspacePanel.width'
const CHANGES_REFRESH_TTL_MS = 1_500
const activeView = ref<PrimaryView | string>('files')
const previewTabs = ref<PreviewTab[]>([])
const roots = ref<TreeNode[]>([])
const changes = ref<GitChange[]>([])
const loadingFiles = ref(false)
const loadingChanges = ref(false)
const workspaceTransitioning = ref(false)
const error = ref('')
const refreshWarning = ref('')
const gitError = ref<import('../../lib/api').GitErrorInfo | null>(null)
const panelWidth = ref(readWorkspacePanelWidth(typeof localStorage === 'undefined' ? null : localStorage.getItem(WIDTH_STORAGE_KEY)))
const resizing = ref(false)
/** 全屏时 macOS 底部（Dock/系统区域）会遮挡面板底部；右侧边栏整体留底，保证终端最后一行可见。 */
const FULLSCREEN_BOTTOM_PAD_PX = 20
const isFullscreen = ref(false)
let unlistenFullscreenResize: (() => void) | undefined
const contextMenu = ref<ContextMenuState | null>(null)
/** Pending delete after context-menu action (Tauri has no usable window.confirm). */
const pendingDelete = ref<{ path: string; name: string; kind: TreeNode['kind'] } | null>(null)
const deletingPath = ref(false)
const isDesktop = isTauriRuntime()
const panelRoot = ref<HTMLElement | null>(null)
const filesScroller = ref<HTMLElement | null>(null)
/** True while the pointer is over the workspace panel (used so ⌘F follows the view under the cursor). */
const pointerOverPanel = ref(false)
const treeSearchOpen = ref(false)
const treeSearchQuery = ref('')
const treeSearchInput = ref<HTMLInputElement | null>(null)
const treeSearchMatches = ref<WorkspaceEntry[]>([])
const treeSearchIndex = ref(0)
const treeSearchLoading = ref(false)
const treeSearchActivePath = ref('')
let treeSearchSeq = 0
let treeSearchDebounce: ReturnType<typeof setTimeout> | null = null
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
const panelStyle = computed(() => ({
  width: `${panelWidth.value}px`,
  ...(isFullscreen.value ? { paddingBottom: `${FULLSCREEN_BOTTOM_PAD_PX}px` } : {})
}))
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
let changesRefreshInFlight: Promise<void> | null = null
let changesRefreshStartedAt = 0
let changesRefreshQueued = false

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
    workspaceTransitioning.value = false
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
    roots.value = silent
      ? mergeWorkspaceTreePreserveState(roots.value, entries)
      : entries.map(entry => ({ ...entry }))
    workspaceTransitioning.value = false
    refreshWarning.value = ''
  } catch (err) {
    if (seq !== rootLoadSeq || props.workspaceRoot !== workspaceRoot) return
    const message = err instanceof Error ? err.message : String(err)
    if (silent) {
      console.warn('[WorkspacePanel] background file tree failed', message)
      refreshWarning.value = '工作区刷新失败，显示的可能不是最新内容'
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
    refreshWarning.value = ''
  } catch (err) {
    if (seq !== changesLoadSeq || props.workspaceRoot !== workspaceRoot) return
    const message = err instanceof Error ? err.message : String(err)
    if (silent) {
      console.warn('[WorkspacePanel] background git status failed', message)
      refreshWarning.value = 'Git 变更刷新失败，显示的可能不是最新内容'
    } else {
      changes.value = []
      error.value = message
    }
  } finally {
    if (!silent && seq === changesLoadSeq) loadingChanges.value = false
  }
}

/** Keep the Changes badge current without overlapping or rapid duplicate git-status requests. */
function refreshChangesBadge(options?: { force?: boolean }) {
  const force = options?.force === true
  const now = Date.now()
  if (changesRefreshInFlight) {
    changesRefreshQueued = changesRefreshQueued || force
    return changesRefreshInFlight
  }
  if (!force && now - changesRefreshStartedAt < CHANGES_REFRESH_TTL_MS) return Promise.resolve()

  changesRefreshStartedAt = now
  changesRefreshInFlight = loadChanges({ silent: true }).finally(() => {
    changesRefreshInFlight = null
    if (!changesRefreshQueued) return
    changesRefreshQueued = false
    void refreshChangesBadge({ force: true })
  })
  return changesRefreshInFlight
}

/** Switch primary nav immediately; do not reload the file tree (keeps scroll + expanded folders). */
function activatePrimaryView(nextView: PrimaryView) {
  const switching = activeView.value !== nextView
  activeView.value = nextView
  if (nextView === 'files') {
    refreshChangesBadge()
    void nextTick(() => {
      filesScroller.value?.focus({ preventScroll: true })
    })
    return
  }
  if (nextView === 'changes') {
    // Force-refresh only when entering Changes from another tab.
    if (switching) void refreshChangesBadge({ force: true })
    return
  }
}

/** Window/tab regained focus — silent badge refresh (respects 1.5s TTL). */
function onWorkspaceFocusRefresh() {
  if (!hasWorkspace.value) return
  if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return
  refreshChangesBadge()
}

function onDocumentVisibilityRefresh() {
  if (document.visibilityState !== 'visible') return
  onWorkspaceFocusRefresh()
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
}

/** Explicit toolbar refresh — shows loading; still non-blocking for the click handler. */
function refreshActiveTab() {
  if (activeView.value === 'terminal') {
    return
  }
  if (activeView.value === 'files') {
    void loadRoot()
    void refreshChangesBadge({ force: true })
  } else if (activeView.value === 'changes') {
    void loadChanges()
  } else if (activeFileTab.value) {
    void loadFileTab(activeFileTab.value)
  } else if (activeDiffTab.value) {
    void loadDiffTab(activeDiffTab.value)
  } else if (activeTurnDiffTab.value) {
    void loadTurnDiffTab(activeTurnDiffTab.value)
  }
}

/** Refresh file tree and change badge after a completed workspace mutation. */
function refreshWorkspaceAfterMutation() {
  void loadRoot({ silent: true })
  void refreshChangesBadge({ force: true })
}

function selectFile(node: Pick<TreeNode, 'kind' | 'name' | 'path'> & { sizeBytes?: number }) {
  if (node.kind !== 'file') return
  const id = workspacePreviewTabId('file', node.path)
  const existing = previewTabs.value.find(item => item.id === id)
  if (existing) {
    // Already open: just focus. Reload only via the toolbar refresh button.
    activeView.value = id
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
  // Switching tabs must not reload — keep cached preview until explicit refresh.
  activeView.value = tabId
}

function closePreviewTabs(targetId: string, action: WorkspaceTabCloseAction = 'close') {
  const ids = previewTabs.value.map(item => item.id)
  const closingIds = workspaceTabIdsToClose(ids, targetId, action)
  const target = previewTabs.value.find(item => item.id === targetId)
  const defaultView: PrimaryView = 'terminal'
  const fallback: PrimaryView = target?.kind === 'diff' || target?.kind === 'turn-diff' ? 'changes' : defaultView
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
  // Extra room for the destructive Delete row + separator.
  contextMenu.value = { kind: 'tree', node, ...menuPosition(event, isDesktop ? 290 : 234) }
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

async function runTreeContextAction(
  action: 'preview' | 'open-system' | 'copy-absolute' | 'copy-relative' | 'reveal' | 'refresh' | 'delete'
) {
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
    else if (action === 'delete') {
      pendingDelete.value = { path: node.path, name: node.name, kind: node.kind }
    } else await runPathAction(node.path, action)
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

function previewTabMatchesDeletedPath(
  tabPath: string,
  deletedPath: string,
  deletedKind: TreeNode['kind']
): boolean {
  if (tabPath === deletedPath) return true
  if (deletedKind !== 'directory') return false
  return tabPath === deletedPath || tabPath.startsWith(`${deletedPath}/`)
}

function closeTabsForDeletedPath(deletedPath: string, deletedKind: TreeNode['kind']) {
  const closing = previewTabs.value.filter(tab =>
    previewTabMatchesDeletedPath(tab.path, deletedPath, deletedKind)
  )
  if (closing.length === 0) return
  const closingIds = new Set(closing.map(tab => tab.id))
  const ids = previewTabs.value.map(item => item.id)
  const fallback: PrimaryView = closing.some(tab => tab.kind === 'diff' || tab.kind === 'turn-diff')
    ? 'changes'
    : 'files'
  const primaryClosedId = closing[0]!.id
  activeView.value = workspaceActiveAfterClose(
    ids,
    activeView.value,
    [...closingIds],
    primaryClosedId,
    fallback
  )
  previewTabs.value = previewTabs.value.filter(item => !closingIds.has(item.id))
}

/** Remove a deleted entry in place so expanded folders stay open. */
function removeNodeFromTree(nodes: TreeNode[], path: string): boolean {
  const index = nodes.findIndex(node => node.path === path)
  if (index >= 0) {
    nodes.splice(index, 1)
    return true
  }
  for (const node of nodes) {
    if (node.children && removeNodeFromTree(node.children, path)) return true
  }
  return false
}

async function confirmPendingDelete() {
  const target = pendingDelete.value
  if (!target || deletingPath.value) return
  deletingPath.value = true
  error.value = ''
  try {
    await deleteWorkspacePath(props.workspaceRoot, target.path)
    console.info('[WorkspacePanel] deleted workspace path', target.path)
    closeTabsForDeletedPath(target.path, target.kind)
    if (!removeNodeFromTree(roots.value, target.path)) {
      await loadRoot({ silent: true })
    }
    pendingDelete.value = null
    void refreshChangesBadge({ force: true })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.warn('[WorkspacePanel] delete workspace path failed', message)
    error.value = message
  } finally {
    deletingPath.value = false
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
  if (event.key !== 'Escape') return
  if (pendingDelete.value) {
    if (!deletingPath.value) pendingDelete.value = null
    return
  }
  if (treeSearchOpen.value) {
    closeTreeSearch()
    return
  }
  closeContextMenu()
}

const treeSearchMatchCountLabel = computed(() => {
  if (!treeSearchQuery.value.trim()) return '0/0'
  if (!treeSearchMatches.value.length) return '0/0'
  return `${treeSearchIndex.value + 1}/${treeSearchMatches.value.length}`
})

function openTreeSearch() {
  treeSearchOpen.value = true
  void nextTick(() => {
    treeSearchInput.value?.focus()
    treeSearchInput.value?.select()
  })
}

function closeTreeSearch() {
  treeSearchOpen.value = false
  treeSearchQuery.value = ''
  treeSearchMatches.value = []
  treeSearchIndex.value = 0
  treeSearchActivePath.value = ''
  treeSearchLoading.value = false
  if (treeSearchDebounce != null) {
    clearTimeout(treeSearchDebounce)
    treeSearchDebounce = null
  }
}

async function ensureTreePathVisible(relativePath: string): Promise<TreeNode | null> {
  const normalized = relativePath.replace(/\\/g, '/')
  for (const dir of workspacePathAncestorDirs(normalized)) {
    let node = findWorkspaceTreeNode(roots.value, dir)
    if (!node) {
      // Parent missing from current tree (e.g. filtered); reload root once.
      await loadRoot({ silent: true })
      node = findWorkspaceTreeNode(roots.value, dir)
    }
    if (!node || node.kind !== 'directory') return null
    if (!node.expanded) node.expanded = true
    if (!node.children) {
      node.loading = true
      try {
        node.children = (await listWorkspaceDirectory(props.workspaceRoot, node.path)).map(
          entry => ({ ...entry })
        )
      } catch (err) {
        node.expanded = false
        console.warn('[WorkspacePanel] expand for search failed', err)
        return null
      } finally {
        node.loading = false
      }
    }
  }
  return findWorkspaceTreeNode(roots.value, normalized)
}

async function focusTreeSearchMatch(index: number) {
  const match = treeSearchMatches.value[index]
  if (!match) return
  treeSearchIndex.value = index
  treeSearchActivePath.value = match.path
  await ensureTreePathVisible(match.path)
  await nextTick()
  const scroller = filesScroller.value
  if (!scroller) return
  const row = scroller.querySelector(
    `[data-workspace-tree-path="${CSS.escape(match.path)}"]`
  ) as HTMLElement | null
  row?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function runTreeSearch(query: string) {
  const q = query.trim()
  const seq = ++treeSearchSeq
  if (!q || !hasWorkspace.value) {
    treeSearchMatches.value = []
    treeSearchIndex.value = 0
    treeSearchActivePath.value = ''
    treeSearchLoading.value = false
    return
  }
  treeSearchLoading.value = true
  try {
    const hits = await searchWorkspaceEntries(props.workspaceRoot, q, 80)
    if (seq !== treeSearchSeq) return
    treeSearchMatches.value = hits
    treeSearchIndex.value = 0
    if (hits.length) await focusTreeSearchMatch(0)
    else treeSearchActivePath.value = ''
  } catch (err) {
    if (seq !== treeSearchSeq) return
    console.warn('[WorkspacePanel] tree search failed', err)
    treeSearchMatches.value = []
    treeSearchActivePath.value = ''
  } finally {
    if (seq === treeSearchSeq) treeSearchLoading.value = false
  }
}

function scheduleTreeSearch(query: string) {
  if (treeSearchDebounce != null) clearTimeout(treeSearchDebounce)
  treeSearchDebounce = setTimeout(() => {
    treeSearchDebounce = null
    void runTreeSearch(query)
  }, 200)
}

function stepTreeSearchMatch(direction: 1 | -1) {
  const count = treeSearchMatches.value.length
  if (!count) return
  const next = (treeSearchIndex.value + direction + count) % count
  void focusTreeSearchMatch(next)
}

function onTreeSearchKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    closeTreeSearch()
  } else if (event.key === 'Enter') {
    event.preventDefault()
    stepTreeSearchMatch(event.shiftKey ? -1 : 1)
  }
}

function isWorkspaceFilesFindTarget(event: KeyboardEvent): boolean {
  if (activeView.value !== 'files' || !hasWorkspace.value) return false
  const panel = panelRoot.value
  if (!panel) return false

  const active = document.activeElement instanceof Element ? document.activeElement : null
  const target = event.target instanceof Element ? event.target : null
  // File preview owns ⌘F while a preview tab is active / focused.
  if (target?.closest('[data-workspace-file-preview]') || active?.closest('[data-workspace-file-preview]')) {
    return false
  }

  // Prefer the pane under the cursor so ⌘F works while browsing files even if
  // the chat composer still holds keyboard focus.
  if (pointerOverPanel.value) return true
  if (active && panel.contains(active)) return true
  if (target && panel.contains(target)) return true
  return false
}

function onGlobalTreeFindShortcut(event: KeyboardEvent) {
  if (event.key.toLocaleLowerCase() !== 'f' || (!event.metaKey && !event.ctrlKey)) return
  if (event.defaultPrevented || !isWorkspaceFilesFindTarget(event)) return
  event.preventDefault()
  event.stopPropagation()
  event.stopImmediatePropagation()
  openTreeSearch()
}

const normalizedWorkspaceRoot = computed(() => props.workspaceRoot.trim().replace(/[\\/]+$/, ''))
const normalizedConversationId = computed(() => props.conversationId?.trim() ?? '')
let loadedWorkspaceRoot = ''
let loadedConversationId = ''

function resetWorkspaceSurface(nextRoot: string, nextConversationId: string) {
  const previousRoot = loadedWorkspaceRoot
  loadedWorkspaceRoot = nextRoot
  loadedConversationId = nextConversationId
  previewTabs.value = []
  roots.value = []
  changes.value = []
  workspaceTransitioning.value = Boolean(nextRoot)
  gitError.value = null
  error.value = ''
  refreshWarning.value = ''
  activeView.value = 'files'
  closeTreeSearch()
  closeContextMenu()
  // Invalidate in-flight loads from the previous workspace.
  rootLoadSeq++
  changesLoadSeq++
  void loadRoot({ silent: true })
  void refreshChangesBadge({ force: true })
}

function onConversationChanged(nextConversationId: string) {
  loadedConversationId = nextConversationId
  // Turn-diff tabs are conversation-scoped; keep file/git tabs for the same workspace.
  previewTabs.value = filterPreviewTabsForConversation(previewTabs.value, nextConversationId)
  if (
    activeView.value !== 'terminal'
    && activeView.value !== 'files'
    && activeView.value !== 'changes'
    && !previewTabs.value.some(tab => tab.id === activeView.value)
  ) {
    activeView.value = 'terminal'
  }
  closeContextMenu()
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
  () => chat.generating,
  (generating, wasGenerating) => {
    if (!wasGenerating || generating || !hasWorkspace.value) return
    const current = chat.current
    if (current?.id !== props.conversationId || !lastTurnFileChanges(current?.messages)) return
    refreshWorkspaceAfterMutation()
  }
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

watch(treeSearchQuery, query => {
  if (!treeSearchOpen.value) return
  scheduleTreeSearch(query)
})

watch(activeView, view => {
  if (view !== 'files' && treeSearchOpen.value) closeTreeSearch()
})

async function refreshFullscreenState() {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    isFullscreen.value = await getCurrentWindow().isFullscreen()
  } catch (err) {
    console.warn('[WorkspacePanel] fullscreen state check failed', err)
  }
}

function watchFullscreenState() {
  if (!isTauriRuntime()) return
  void refreshFullscreenState()
  void import('@tauri-apps/api/window')
    .then(({ getCurrentWindow }) => getCurrentWindow().onResized(() => { void refreshFullscreenState() }))
    .then(unlisten => { unlistenFullscreenResize = unlisten })
    .catch(err => console.warn('[WorkspacePanel] fullscreen resize listener failed', err))
}

onMounted(() => {
  watchFullscreenState()
  window.addEventListener('resize', handleViewportResize)
  window.addEventListener('scroll', closeContextMenu, true)
  document.addEventListener('keydown', handleDocumentKeydown)
  window.addEventListener('keydown', onGlobalTreeFindShortcut, true)
  window.addEventListener('focus', onWorkspaceFocusRefresh)
  document.addEventListener('visibilitychange', onDocumentVisibilityRefresh)
  const pending = workspacePanelStore.consumePendingTurnDiff()
  if (pending) void openTurnDiff(pending.conversationId, pending.turnId, pending.path)
})

onBeforeUnmount(() => {
  finishResize()
  unlistenFullscreenResize?.()
  window.removeEventListener('resize', handleViewportResize)
  window.removeEventListener('scroll', closeContextMenu, true)
  document.removeEventListener('keydown', handleDocumentKeydown)
  window.removeEventListener('keydown', onGlobalTreeFindShortcut, true)
  window.removeEventListener('focus', onWorkspaceFocusRefresh)
  document.removeEventListener('visibilitychange', onDocumentVisibilityRefresh)
  if (treeSearchDebounce != null) clearTimeout(treeSearchDebounce)
})
</script>

<template>
  <aside
    ref="panelRoot"
    class="workspace-panel hidden lg:flex shrink-0 flex-col min-h-0 overflow-hidden border-l border-border bg-card relative"
    data-workspace-panel
    :class="resizing && 'is-resizing'"
    :style="panelStyle"
    @pointerenter="pointerOverPanel = true"
    @pointerleave="pointerOverPanel = false"
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
      <button
        type="button"
        class="workspace-tab workspace-tab-icon shrink-0"
        :class="activeView === 'terminal' && 'is-active'"
        title="调试终端"
        aria-label="调试终端"
        @click="activatePrimaryView('terminal')"
      >
        <SquareTerminal class="w-3.5 h-3.5" />
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

    <template v-else>
      <TerminalPanel
        v-if="activeView === 'terminal'"
        :workspace-root="workspaceRoot"
        :conversation-id="conversationId ?? ''"
        active
      />
      <div v-if="refreshWarning" class="workspace-refresh-warning" role="status">{{ refreshWarning }}</div>
      <!-- Keep Files / Changes mounted so scroll + expanded folders survive tab switches. -->
      <div
        v-show="activeView === 'files'"
        ref="filesScroller"
        class="workspace-scroll-area relative flex-1 min-h-0 overflow-auto p-2 outline-none"
        tabindex="-1"
      >
        <div
          v-if="treeSearchOpen"
          class="sticky top-0 z-20 mx-2 mb-2 flex items-center gap-1 rounded-lg border border-border bg-background/95 p-1.5 shadow-lg backdrop-blur"
          role="search"
        >
          <Search class="ml-1 h-3.5 w-3.5 shrink-0 text-muted" aria-hidden="true" />
          <input
            ref="treeSearchInput"
            v-model="treeSearchQuery"
            class="w-full min-w-0 bg-transparent px-1.5 py-1 text-xs text-foreground outline-none placeholder:text-muted"
            type="search"
            placeholder="查找文件"
            aria-label="查找工作区文件"
            @keydown="onTreeSearchKeydown"
          />
          <span class="min-w-10 shrink-0 text-center text-[10px] tabular-nums text-muted">
            <Loader2 v-if="treeSearchLoading" class="mx-auto h-3 w-3 animate-spin" />
            <template v-else>{{ treeSearchMatchCountLabel }}</template>
          </span>
          <button
            type="button"
            class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
            title="上一个（Shift+Enter）"
            :disabled="!treeSearchMatches.length"
            @click="stepTreeSearchMatch(-1)"
          >
            <ArrowUp class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
            title="下一个（Enter）"
            :disabled="!treeSearchMatches.length"
            @click="stepTreeSearchMatch(1)"
          >
            <ArrowDown class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground"
            title="关闭（Esc）"
            @click="closeTreeSearch"
          >
            <X class="h-3.5 w-3.5" />
          </button>
        </div>
        <div v-if="loadingFiles || workspaceTransitioning" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载工作区…</div>
        <div v-else-if="!roots.length" class="workspace-empty">目录为空</div>
        <WorkspaceTreeNode
          v-for="node in roots"
          :key="node.path"
          :node="node"
          :highlighted-path="contextSelectedTreePath || treeSearchActivePath"
          @toggle="toggleDirectory"
          @activate="selectFile"
          @contextmenu="openTreeContextMenu"
        />
      </div>

      <div v-show="activeView === 'changes'" class="workspace-scroll-area flex-1 min-h-0 overflow-auto p-2">
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

      <div v-if="activeFileTab && activeView === activeFileTab.id" class="flex-1 min-h-0 overflow-hidden p-2">
        <div v-if="activeFileTab.loading" class="workspace-empty"><Loader2 class="w-4 h-4 animate-spin" /> 加载文件…</div>
        <div v-else-if="activeFileTab.error" class="text-xs text-danger break-words p-2">{{ activeFileTab.error }}</div>
        <WorkspaceFilePreview
          v-else-if="activeFileTab.preview"
          :preview="activeFileTab.preview"
          :absolute-path="workspaceAbsolutePath(workspaceRoot, activeFileTab.path)"
          :workspace-root="workspaceRoot"
          :relative-path="activeFileTab.path"
          @open-reference="openMarkdownReference"
        />
        <div v-else class="workspace-empty">无法显示该文件</div>
      </div>

      <div v-else-if="activeTurnDiffTab && activeView === activeTurnDiffTab.id" class="flex-1 min-h-0 overflow-hidden p-2">
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

      <div v-else-if="activeDiffTab && activeView === activeDiffTab.id" class="flex-1 min-h-0 overflow-hidden p-2">
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
      <div
        v-else-if="activeView !== 'terminal' && activeView !== 'files' && activeView !== 'changes'"
        class="workspace-empty"
      >预览标签已关闭</div>
    </template>

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
            <div class="workspace-context-separator" />
            <button type="button" role="menuitem" class="is-danger" @click="runTreeContextAction('delete')"><Trash2 />删除</button>
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

    <Teleport to="body">
      <div
        v-if="pendingDelete"
        class="fixed inset-0 z-[310] flex items-center justify-center bg-[hsl(var(--foreground)/0.32)] p-4"
        @click.self="!deletingPath && (pendingDelete = null)"
      >
        <section class="w-full max-w-sm rounded-xl border border-border bg-card p-5" role="dialog" aria-modal="true" aria-label="确认删除">
          <h2 class="text-base font-semibold text-foreground">
            {{ pendingDelete.kind === 'directory' ? '删除文件夹？' : '删除文件？' }}
          </h2>
          <p class="mt-2 text-sm leading-6 text-muted">
            {{ pendingDelete.kind === 'directory'
              ? '将永久删除该文件夹及其全部内容，此操作不可撤销。'
              : '将永久删除该文件，此操作不可撤销。' }}
          </p>
          <div class="mt-3 rounded-lg border border-border bg-hover/50 px-3 py-2">
            <div class="text-[11px] font-medium text-muted">路径</div>
            <div class="mt-0.5 break-all font-mono text-xs text-foreground">{{ pendingDelete.path }}</div>
          </div>
          <div class="mt-5 flex justify-end gap-2">
            <button
              type="button"
              class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover disabled:opacity-50"
              :disabled="deletingPath"
              @click="pendingDelete = null"
            >取消</button>
            <button
              type="button"
              class="rounded-md bg-danger px-3 py-1.5 text-sm text-white hover:opacity-90 disabled:opacity-50"
              :disabled="deletingPath"
              @click="confirmPendingDelete"
            >{{ deletingPath ? '删除中…' : '删除' }}</button>
          </div>
        </section>
      </div>
    </Teleport>
  </aside>
</template>

<style scoped>
.workspace-panel.is-resizing { @apply select-none; }
.workspace-scroll-area { scrollbar-gutter: stable; }
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
.workspace-refresh-warning { @apply shrink-0 border-b border-amber-300/40 bg-amber-50 px-3 py-1.5 text-[11px] text-amber-800 dark:border-amber-500/30 dark:bg-amber-950/30 dark:text-amber-200; }
.workspace-action-btn { @apply rounded border border-border px-2.5 py-1.5 text-xs text-foreground hover:bg-hover disabled:opacity-50; }
.change-row { @apply w-full flex items-center gap-1.5 px-3 py-1.5 text-left text-xs hover:bg-hover; }
.change-row.is-selected { @apply bg-accent/10 text-foreground; }
.status-badge { @apply min-w-7 text-center font-mono text-[10px] text-accent; }
.workspace-context-menu { @apply fixed z-[301] w-52 rounded-md border border-border bg-card p-1 shadow-xl select-none; }
.workspace-context-separator { @apply my-1 border-t border-border; }
.workspace-context-menu button { @apply w-full flex items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground hover:bg-hover; }
.workspace-context-menu button :deep(svg) { @apply w-3.5 h-3.5 text-muted; }
.workspace-context-menu button.is-danger { @apply text-danger; }
.workspace-context-menu button.is-danger :deep(svg) { @apply text-danger; }
</style>
