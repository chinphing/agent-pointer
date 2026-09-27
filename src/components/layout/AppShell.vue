<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, watch, nextTick, defineAsyncComponent, provide } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  Plus,
  Search,
  MessageSquare,
  Loader2,
  Trash2,
  Check,
  X,
  PanelLeftClose,
  PanelRightOpen,
  FolderGit2,
  FolderOpen,
  Clock3,
  Sparkles,
  Link2,
  ChevronDown,
  ChevronRight,
  MoreHorizontal,
  Pin,
  PinOff,
  Pencil,
  Copy,
  Settings2
} from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useWorkspacePanelStore } from '../../stores/workspacePanel'
import {
  createProject, searchConversations, listConversationSearchMatches, updateProject, revealInFinder
} from '../../lib/api'
import { GIT_INITIALIZATION_TASK } from '../../lib/workspacePanel'
import { applyProjectCreationResult, projectNameFromWorkspaceRoot } from '../../lib/projectCreation'
import SkillDirectoryPicker from '../skills/SkillDirectoryPicker.vue'
import { useWindowChrome } from '../../composables/useWindowChrome'
import { useSidebarCollapse } from '../../composables/useSidebarCollapse'
import { useSidebarProjectExpand } from '../../composables/useSidebarProjectExpand'
import { showScrollbarWhileScrolling } from '../../lib/autoHideScrollbar'
import { useSidebarSectionCollapse } from '../../composables/useSidebarSectionCollapse'
import WindowControls from './WindowControls.vue'
import WindowDragRegion from './WindowDragRegion.vue'
import DesktopSnapshotButton from './DesktopSnapshotButton.vue'
import AccountMenu from './AccountMenu.vue'
import ChatTopBar from '../chat/ChatTopBar.vue'
import { isTauriRuntime } from '../../lib/runtime'
import {
  resolveBrandIcon,
  resolveBrandName,
  resolveDesktopSnapshotEnabled
} from '../../lib/webBranding'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'
import { OpenSettingsKey } from '../../lib/settingsDialogKey'
import type { Project } from '../../types/chat'

/** Lazy: DiffView + markdown preview stay out of the first paint. */
const WorkspacePanel = defineAsyncComponent(() => import('../workspace/WorkspacePanel.vue'))

// Warm up the lazy chunks in idle time: dev mode compiles the module graph eagerly
// (otherwise the first panel open / Terminal tab switch waits for Vite to compile
// DiffView + preview + markdown + xterm on demand, which can take ~1s+), and prod
// preloads the chunks.
const warmWorkspacePanel = () => {
  void import('../workspace/WorkspacePanel.vue')
  // Terminal is the other lazy tab (xterm is a large dependency) — pre-compile it
  // so the first Terminal switch does not block on Vite on-demand compilation.
  void import('../workspace/TerminalPanel.vue')
}
if (typeof requestIdleCallback === 'function') {
  requestIdleCallback(warmWorkspacePanel, { timeout: 2000 })
} else {
  setTimeout(warmWorkspacePanel, 1200)
}

const emit = defineEmits<{
  (e: 'open-settings', section?: string): void
  (e: 'open-automation'): void
}>()

// Chat UI (ChatTopBar / Composer) opens the settings dialog through this.
const { t } = useI18n()
const chat = useChatStore()
const workspacePanel = useWorkspacePanelStore()
const platformAuth = usePlatformAuthStore()
const settingsStore = useSettingsStore()

const brandName = resolveBrandName()
const brandIcon = resolveBrandIcon()
const desktopSnapshotEnabled = resolveDesktopSnapshotEnabled()
/** Platform / standalone admin — gates settings entry and workspace panel. */
const isAppAdmin = computed(
  () => platformAuth.isPlatformAdmin || settingsStore.isPlatformAdmin
)

provide(OpenSettingsKey, (section?: string) => {
  if (!isAppAdmin.value) return
  emit('open-settings', section)
})

const { collapsed: sidebarCollapsed, toggle: toggleSidebar } = useSidebarCollapse()
const {
  isExpanded: isProjectExpanded,
  setExpanded: setProjectExpanded,
  toggleExpanded: toggleProjectIdExpanded,
  forgetProject: forgetExpandedProject
} = useSidebarProjectExpand()
const {
  sectionCollapse,
  togglePinned: togglePinnedSectionState,
  toggleProjects: toggleProjectsSectionState,
  toggleConversations: toggleConversationsSectionState,
  expandProjects,
  expandConversations
} = useSidebarSectionCollapse()
const pinnedSectionCollapsed = computed(() => sectionCollapse.value.pinned)
const projectsSectionCollapsed = computed(() => sectionCollapse.value.projects)
const conversationsSectionCollapsed = computed(() => sectionCollapse.value.conversations)
const workspacePanelOpen = computed(() => workspacePanel.open)

function setWorkspacePanelOpen(open: boolean) {
  if (!isAppAdmin.value) {
    workspacePanel.setOpen(false)
    return
  }
  workspacePanel.setOpen(open)
}

watch(isAppAdmin, admin => {
  if (!admin) workspacePanel.setOpen(false)
})

/**
 * Two-step inline delete confirmation. The first click on the trash icon
 * reveals inline 确认/取消 buttons; only the second click (确认) actually
 * deletes. We avoid `window.confirm` because Tauri's webview does not render
 * native browser dialogs — `window.confirm` returns true without any UI,
 * making a naive guard a no-op on desktop.
 */
const pendingDeleteId = ref<string | null>(null)

/** Rename dialog (opened by double-clicking a conversation title). */
const renameTarget = ref<{ id: string; title: string } | null>(null)
const renameTitle = ref('')
const renameInputRef = ref<HTMLInputElement | null>(null)
const renameComposing = ref(false)

function startEdit(conv: { id: string; title: string }) {
  pendingDeleteId.value = null
  renameTarget.value = { id: conv.id, title: conv.title }
  renameTitle.value = conv.title
  nextTick(() => {
    renameInputRef.value?.focus()
    renameInputRef.value?.select()
  })
}

function saveRename() {
  if (!renameTarget.value) return
  const newTitle = renameTitle.value.trim()
  if (newTitle) {
    chat.renameConversation(renameTarget.value.id, newTitle)
  }
  renameTarget.value = null
  renameTitle.value = ''
}

/** Enter saves — but never while an IME is composing (Chinese candidate confirm). */
function onRenameEnter(event: KeyboardEvent) {
  if (event.isComposing || renameComposing.value) return
  event.preventDefault()
  saveRename()
}

/**
 * Keep guarding a short window after compositionend: some IMEs (e.g. English
 * candidates) deliver the confirming Enter AFTER compositionend, when
 * `isComposing` is already false — mirroring Composer's onCompositionEnd.
 */
function onRenameCompositionEnd() {
  setTimeout(() => {
    renameComposing.value = false
  }, 50)
}

function cancelRename() {
  renameTarget.value = null
  renameTitle.value = ''
}

/** ESC closes rename dialog without saving. */
function onRenameKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    cancelRename()
  }
}

function askDeleteConversation(c: { id: string }) {
  pendingDeleteId.value = c.id
}

function cancelDeleteConversation() {
  pendingDeleteId.value = null
}

function confirmDeleteConversation(c: { id: string }) {
  pendingDeleteId.value = null
  chat.deleteConversation(c.id)
}

/** Row click selects the conversation and dismisses any pending delete. */
function onRowClick(c: {
  id: string
  title?: string
  updatedAt?: number
  messageId?: string
  messageCount?: number
  projectId?: string
}) {
  pendingDeleteId.value = null
  chat.openConversation(c.id, {
    focusMessageId: c.messageId?.trim() || undefined,
    focusQueryTerm: searchQuery.value.trim() || undefined,
    ensureShell: {
      title: c.title,
      updatedAt: c.updatedAt,
      messageCount: c.messageCount,
      projectId: c.projectId
    }
  })
}

const projectSearchQuery = ref('')
const projectSearchExpanded = ref(false)
const projectSearchInputRef = ref<HTMLInputElement | null>(null)
const conversationSearchExpanded = ref(false)
const conversationSearchInputRef = ref<HTMLInputElement | null>(null)
const showProjectCreator = ref(false)
const sidebarProjects = computed(() => {
  const query = projectSearchQuery.value.trim().toLocaleLowerCase()
  return chat.projects.filter(project =>
    !project.isArchived
    && (!query
      || displayProjectName(project).toLocaleLowerCase().includes(query)
      || project.workspaceRoot.toLocaleLowerCase().includes(query))
  )
})
const projectCursors = ref<Record<string, import('../../types/chat').ConversationCursor | null>>({})
const projectLoading = ref(new Set<string>())
const projectName = ref('')
const projectRoot = ref('')
const projectError = ref('')
const projectEditing = ref<Project | null>(null)
const projectPendingDeletion = ref<Project | null>(null)
const projectMenuId = ref<string | null>(null)
const projectMenuPosition = ref({ left: 0, top: 0 })

function displayProjectName(project: Project): string {
  return project.isDefault ? t('shell.defaultProject') : project.name
}

function toggleProjectMenu(project: Project, event: MouseEvent) {
  if (projectMenuId.value === project.id) {
    projectMenuId.value = null
    return
  }
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
  projectMenuPosition.value = {
    left: Math.max(8, rect.right - 160),
    top: rect.bottom + 4
  }
  projectMenuId.value = project.id
}

async function pickProjectDirectory() {
  if (!isTauriRuntime()) {
    projectError.value = t('shell.directoryPickerUnsupported')
    return
  }
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: false })
    if (typeof selected === 'string' && selected.trim()) projectRoot.value = selected
  } catch (err) {
    projectError.value = String(err)
  }
}

async function addProject() {
  const name = projectName.value.trim()
  const root = projectRoot.value.trim()
  if (!name || !root) {
    projectError.value = t('shell.projectNameAndDirectoryRequired')
    return
  }
  try {
    const result = await createProject(name, root)
    await applyProjectCreationResult(result, {
      refreshProjects: chat.refreshProjects,
      selectProject: chat.switchProject,
      notify: message => chat.showUiToast(message, 'warning')
    })
    projectName.value = ''
    projectRoot.value = ''
    projectError.value = ''
    showProjectCreator.value = false
  } catch (err) {
    projectError.value = String(err)
  }
}

/** 点击技能目录卡片：直接以技能名+目录创建项目，几乎零思考。 */
async function createProjectFromSkill(dir: { name: string; path: string }) {
  projectName.value = dir.name
  projectRoot.value = dir.path
  projectError.value = ''
  await addProject()
}

/** 粘贴路径时自动补全项目名称（若名称仍为空），让创建按钮可直接点击。 */
watch(projectRoot, root => {
  const trimmed = root.trim()
  if (!trimmed || projectName.value.trim()) return
  if (trimmed.includes('/') || trimmed.includes('\\') || trimmed.startsWith('~')) {
    projectName.value = projectNameFromWorkspaceRoot(trimmed)
  }
})

async function persistProject(project: Project) {
  try {
    const updated = await updateProject(project.id, {
      name: project.name, workspaceRoot: project.workspaceRoot,
      isPinned: project.isPinned, isArchived: project.isArchived
    })
    chat.projects = (chat.projects
      .map(p => p.id === updated.id ? updated : p)
      .sort((a, b) => Number(b.isPinned) - Number(a.isPinned)
        || b.lastActivityAt - a.lastActivityAt
        || b.id.localeCompare(a.id)))
    await chat.refreshProjects()
  } catch (err) {
    projectError.value = String(err)
  }
}

async function toggleProjectPin(project: Project) {
  projectMenuId.value = null
  project.isPinned = !project.isPinned
  await persistProject(project)
}

async function openProjectSettings(project: Project) {
  projectMenuId.value = null
  projectEditing.value = { ...project }
}

async function confirmProjectDeletion() {
  const project = projectPendingDeletion.value
  if (!project) return
  try {
    await chat.deleteProject(project.id)
    forgetExpandedProject(project.id)
    projectPendingDeletion.value = null
  } catch (err) {
    projectError.value = String(err)
  }
}

async function loadMoreProjects() {
  if (chat.loadingMoreProjects || !chat.hasMoreProjects) return
  try {
    await chat.loadMoreProjects()
  } catch (err) {
    projectError.value = String(err)
  }
}

function requestDeleteProject(project: Project) {
  projectMenuId.value = null
  projectPendingDeletion.value = project
}

function newTask(project?: Project) {
  const conversation = project
    ? chat.newConversation(project.id, project.workspaceRoot)
    : chat.newConversation()
  searchQuery.value = ''
  nextTick(() => listScroller.value?.scrollTo({ top: 0, behavior: 'smooth' }))
  return conversation
}

function projectConversations(project: Project) {
  return visibleConversations(chat.conversations)
    .filter(conversation => conversation.projectId === project.id && !conversation.isPinned)
    .slice()
    .sort(compareUnpinnedByActivity)
}

async function ensureProjectConversationsLoaded(project: Project) {
  if (!projectCursors.value[project.id] && projectConversations(project).length === 0) {
    await loadMoreProjectConversations(project)
  }
}

/** Chevron only: expand / collapse without changing the active project. */
async function onProjectChevronClick(project: Project) {
  const expanded = toggleProjectIdExpanded(project.id)
  if (expanded) await ensureProjectConversationsLoaded(project)
}

/** Row body: select project; expand if currently collapsed, never collapse. */
async function selectProject(project: Project) {
  if (!isProjectExpanded(project.id)) {
    setProjectExpanded(project.id, true)
    await ensureProjectConversationsLoaded(project)
  }
  await chat.switchProject(project.id)
}

async function loadMoreProjectConversations(project: Project) {
  if (projectLoading.value.has(project.id)) return
  projectLoading.value = new Set([...projectLoading.value, project.id])
  try {
    const page = await chat.loadProjectConversations(project.id, projectCursors.value[project.id] ?? null)
    projectCursors.value = { ...projectCursors.value, [project.id]: page.nextCursor }
  } catch (err) {
    console.error('[sidebar] load project conversations failed', project.id, err)
  } finally {
    const next = new Set(projectLoading.value)
    next.delete(project.id)
    projectLoading.value = next
  }
}

function openAutomation() {
  emit('open-automation')
}

function openProjectSearch() {
  closeConversationSearch()
  expandProjects()
  projectSearchExpanded.value = true
  nextTick(() => projectSearchInputRef.value?.focus())
}

function closeProjectSearch() {
  projectSearchExpanded.value = false
  projectSearchQuery.value = ''
}

function openConversationSearch() {
  closeProjectSearch()
  expandConversations()
  conversationSearchExpanded.value = true
  nextTick(() => conversationSearchInputRef.value?.focus())
}

function closeConversationSearch() {
  conversationSearchExpanded.value = false
  searchQuery.value = ''
}

function togglePinnedSection() {
  togglePinnedSectionState()
}

function toggleProjectsSection() {
  toggleProjectsSectionState()
  if (sectionCollapse.value.projects) closeProjectSearch()
}

function toggleConversationsSection() {
  toggleConversationsSectionState()
  if (sectionCollapse.value.conversations) closeConversationSearch()
}

function closeProjectMenuOnOutsideClick(event: MouseEvent) {
  const target = event.target
  if (!(target instanceof Element)) return
  if (!target.closest('.sidebar-project-section')) {
    closeProjectSearch()
  }
  if (!target.closest('.sidebar-conversation-section')) closeConversationSearch()
  if (!target.closest('.project-context-menu, .project-menu-trigger')) {
    projectMenuId.value = null
  }
  if (!target.closest('.conversation-context-menu')) {
    closeConversationMenu()
  }
}

function closeProjectOverlaysOnKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  if (conversationMenu.value) {
    closeConversationMenu()
    return
  }
  if (conversationSearchExpanded.value) {
    closeConversationSearch()
    return
  }
  if (projectSearchExpanded.value) {
    closeProjectSearch()
    return
  }
  projectMenuId.value = null
  if (projectPendingDeletion.value) {
    projectPendingDeletion.value = null
    return
  }
  if (projectEditing.value) projectEditing.value = null
  if (showProjectCreator.value) showProjectCreator.value = false
  if (renameTarget.value) cancelRename()
}
const {
  enabled: chromeEnabled,
  os,
  maximized,
  showCustomControls,
  macTrafficLightPadding,
  minimize,
  toggleMaximize,
  close: closeWindow
} = useWindowChrome()

/** Windows / Linux: min/max/close on main top-right. */
const useMainAreaWindowControls = computed(
  () => showCustomControls.value && (os.value === 'windows' || os.value === 'linux')
)

const searchQuery = ref('')
const searchLoading = ref(false)
type SidebarRow = {
  id: string
  title: string
  updatedAt: number
  isPinned?: boolean
  snippet?: string
  messageId?: string
  messageCount?: number
  projectId?: string
  matches?: { messageId: string; role?: string; snippet: string }[]
  matchCount?: number
}
const searchResults = ref<SidebarRow[]>([])
const expandedSearchId = ref<string | null>(null)

function matchRoleLabel(role?: string): string {
  switch ((role || '').trim().toLowerCase()) {
    case 'user':
      return t('shell.roleUser')
    case 'assistant':
      return t('shell.roleAssistant')
    case 'tool':
      return t('shell.roleTool')
    default:
      return t('shell.roleMessage')
  }
}

async function toggleSearchMatches(conversationId: string) {
  if (expandedSearchId.value === conversationId) {
    expandedSearchId.value = null
    return
  }
  const row = searchResults.value.find(r => r.id === conversationId)
  const q = searchQuery.value.trim()
  if (row && q && (row.matchCount ?? 0) > (row.matches?.length ?? 0)) {
    try {
      const matches = await listConversationSearchMatches(conversationId, q)
      const normalized = matches
        .filter(m => m.messageId.trim() && m.snippet.trim())
        .map(m => ({
          messageId: m.messageId.trim(),
          role: m.role,
          snippet: m.snippet.trim()
        }))
      searchResults.value = searchResults.value.map(item =>
        item.id === conversationId
          ? {
              ...item,
              matches: normalized,
              matchCount:
                normalized.length > 0 ? normalized.length : item.matchCount
            }
          : item
      )
    } catch (err) {
      console.error('[sidebar] listConversationSearchMatches failed', err)
    }
  }
  expandedSearchId.value = conversationId
}

function onMatchClick(
  c: SidebarRow,
  match: { messageId: string },
  event: Event
) {
  event.stopPropagation()
  pendingDeleteId.value = null
  chat.openConversation(c.id, {
    focusMessageId: match.messageId.trim() || undefined,
    focusQueryTerm: searchQuery.value.trim() || undefined,
    ensureShell: {
      title: c.title,
      updatedAt: c.updatedAt,
      messageCount: c.messageCount,
      projectId: c.projectId
    }
  })
}
let searchTimer: ReturnType<typeof setTimeout> | null = null
let searchSeq = 0

function visibleConversations(list: typeof chat.conversations) {
  const activeId = chat.currentId
  return list.filter(c => {
    const isolated = c.id.startsWith('cron:') || c.id.startsWith('webhook:')
    return !isolated || c.id === activeId
  })
}

function compareUnpinnedByActivity(
  a: { updatedAt: number; id: string },
  b: { updatedAt: number; id: string }
): number {
  return b.updatedAt - a.updatedAt || b.id.localeCompare(a.id)
}

function toggleConversationPin(c: { id: string }) {
  chat.toggleConversationPin(c.id)
}

type ConversationMenuTarget = {
  id: string
  title: string
  isPinned: boolean
  workspaceRoot: string
}

const conversationMenu = ref<{
  target: ConversationMenuTarget
  left: number
  top: number
} | null>(null)

function resolveConversationWorkspaceRoot(conversationId: string): string {
  const conv = chat.conversations.find(c => c.id === conversationId)
  const direct = conv?.workspaceRoot?.trim()
  if (direct) return direct
  const projectId = conv?.projectId?.trim()
  if (!projectId) return ''
  return chat.projectById(projectId)?.workspaceRoot?.trim() || ''
}

function openConversationMenu(event: MouseEvent, c: { id: string; title?: string }) {
  event.preventDefault()
  event.stopPropagation()
  projectMenuId.value = null
  pendingDeleteId.value = null
  const full = chat.conversations.find(item => item.id === c.id)
  const menuWidth = 188
  const menuHeight = 220
  const left = Math.min(Math.max(8, event.clientX), window.innerWidth - menuWidth - 8)
  const top = Math.min(Math.max(8, event.clientY), window.innerHeight - menuHeight - 8)
  conversationMenu.value = {
    target: {
      id: c.id,
      title: full?.title?.trim() || c.title?.trim() || t('shell.session'),
      isPinned: !!full?.isPinned,
      workspaceRoot: resolveConversationWorkspaceRoot(c.id)
    },
    left,
    top
  }
}

function closeConversationMenu() {
  conversationMenu.value = null
}

async function copyText(label: string, text: string) {
  const value = text.trim()
  if (!value) {
    chat.showUiToast(t('shell.copyEmptyLabel', { label }), 'warning')
    return
  }
  try {
    await navigator.clipboard.writeText(value)
    chat.showUiToast(t('shell.copiedLabel', { label }), 'success')
  } catch (err) {
    console.error('[sidebar] clipboard write failed', label, err)
    chat.showUiToast(t('shell.copyFailedLabel', { label }), 'error')
  }
}

async function onConversationMenuPin() {
  const target = conversationMenu.value?.target
  closeConversationMenu()
  if (!target) return
  toggleConversationPin(target)
}

function onConversationMenuRename() {
  const target = conversationMenu.value?.target
  closeConversationMenu()
  if (!target) return
  startEdit(target)
}

async function onConversationMenuCopyId() {
  const target = conversationMenu.value?.target
  closeConversationMenu()
  if (!target) return
  await copyText(t('shell.sessionId'), target.id)
}

async function onConversationMenuCopyWorkspace() {
  const target = conversationMenu.value?.target
  closeConversationMenu()
  if (!target) return
  await copyText(t('shell.workspaceDirectory'), target.workspaceRoot)
}

async function onConversationMenuRevealWorkspace() {
  const target = conversationMenu.value?.target
  closeConversationMenu()
  if (!target) return
  const root = target.workspaceRoot.trim()
  if (!root) {
    chat.showUiToast(t('shell.workspaceEmpty'), 'warning')
    return
  }
  if (!isTauriRuntime()) {
    chat.showUiToast(t('shell.finderNotSupportedWeb'), 'warning')
    return
  }
  try {
    await revealInFinder(root)
  } catch (err) {
    console.error('[sidebar] revealInFinder failed', root, err)
    chat.showUiToast(t('shell.openDirectoryFailed'), 'error')
  }
}

const pinnedConversations = computed(() =>
  visibleConversations(chat.conversations)
    .filter(c => !!c.isPinned)
    .slice()
    .sort(compareUnpinnedByActivity)
)

const sidebarRows = computed((): SidebarRow[] => {
  if (searchQuery.value.trim()) return searchResults.value
  return visibleConversations(chat.conversations)
    .filter(c => !c.isPinned)
    .slice()
    .sort(compareUnpinnedByActivity)
    .map(c => ({
      id: c.id,
      title: c.title,
      updatedAt: c.updatedAt,
      isPinned: false
    }))
})

async function runSidebarSearch(query: string) {
  const seq = ++searchSeq
  searchLoading.value = true
  try {
    const hits = await searchConversations(query, 50)
    if (seq !== searchSeq) return
    const activeId = chat.currentId
    searchResults.value = hits
      .filter(h => {
        const isolated = h.id.startsWith('cron:') || h.id.startsWith('webhook:')
        return !isolated || h.id === activeId
      })
      .map(h => ({
        id: h.id,
        title: h.title,
        updatedAt: h.updatedAt,
        // Prefer FTS match-centered snippet; preview is only a last-resort fallback.
        snippet: h.snippet?.trim() || h.preview?.trim() || undefined,
        messageId: h.messageId?.trim() || undefined,
        messageCount: h.messageCount,
        projectId: h.projectId,
        matches: h.matches
          ?.filter(m => m.messageId.trim() && m.snippet.trim())
          .map(m => ({
            messageId: m.messageId.trim(),
            role: m.role,
            snippet: m.snippet.trim()
          })),
        matchCount: h.matchCount
      }))
  } catch (err) {
    console.error('[sidebar] searchConversations failed', err)
    if (seq === searchSeq) searchResults.value = []
  } finally {
    if (seq === searchSeq) searchLoading.value = false
  }
}

// --- Sidebar infinite scroll (cursor-paginated conversation metas) ---
const listScroller = ref<HTMLElement | null>(null)
const sentinel = ref<HTMLElement | null>(null)
let observer: IntersectionObserver | null = null

function maybeLoadMore(entry: IntersectionObserverEntry) {
  if (!entry.isIntersecting) return
  if (searchQuery.value.trim()) return
  if (!chat.hasMoreConversations || chat.loadingMoreConversations) return
  void chat.loadMoreConversations()
}

onMounted(() => {
  document.addEventListener('mousedown', closeProjectMenuOnOutsideClick)
  document.addEventListener('keydown', closeProjectOverlaysOnKeydown)
  if (!sentinel.value || !listScroller.value) return
  observer = new IntersectionObserver(
    entries => {
      for (const e of entries) maybeLoadMore(e)
    },
    { root: listScroller.value, rootMargin: '120px' }
  )
  observer.observe(sentinel.value)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', closeProjectMenuOnOutsideClick)
  document.removeEventListener('keydown', closeProjectOverlaysOnKeydown)
  observer?.disconnect()
  observer = null
})

// When the sidebar expands after being collapsed, the scroll container may
// re-become interactive; re-arm the observer so a pending next page fires.
watch(sidebarCollapsed, collapsed => {
  if (collapsed || !observer || !sentinel.value) return
  observer.disconnect()
  if (listScroller.value) {
    observer = new IntersectionObserver(
      entries => {
        for (const e of entries) maybeLoadMore(e)
      },
      { root: listScroller.value, rootMargin: '120px' }
    )
    observer.observe(sentinel.value)
  }
})

// Dismiss the inline delete confirmation and rename dialog when the active
// conversation or search filter changes, so a stale pending state never lingers.
watch(() => chat.currentId, () => {
  pendingDeleteId.value = null
  cancelRename()
})
// Dismiss rename dialog when sidebar collapses.
watch(sidebarCollapsed, (collapsed) => {
  if (collapsed) cancelRename()
})
watch(searchQuery, q => {
  pendingDeleteId.value = null
  expandedSearchId.value = null
  if (searchTimer) clearTimeout(searchTimer)
  const trimmed = q.trim()
  if (!trimmed) {
    searchSeq++
    searchResults.value = []
    searchLoading.value = false
    return
  }
  // Mark loading immediately (before debounce). Otherwise the list switches to
  // empty searchResults while searchLoading is still false, flashing「没有找到匹配的会话」.
  searchLoading.value = true
  searchTimer = setTimeout(() => void runSidebarSearch(trimmed), 300)
})
</script>

<template>
  <div class="h-full w-full flex flex-col min-h-0">
    <div class="flex flex-1 min-h-0">
      <aside
        class="app-sidebar shell-sidebar hidden md:flex shrink-0 flex-col border-r border-border transition-[width] duration-200 ease-out overflow-hidden"
        :class="sidebarCollapsed ? 'w-0 border-r-0' : 'w-[260px]'"
      >
        <!-- A: 侧栏顶栏 -->
        <WindowDragRegion
          v-if="!sidebarCollapsed"
          region="sidebar-top-chrome"
          class="sidebar-chrome shrink-0 flex items-center gap-1 pr-2 select-none bg-transparent whitespace-nowrap"
          :class="chromeEnabled && macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
        >
          <!-- macOS 红绿灯覆盖时先留出系统按钮空间，再放品牌 -->
          <div
            v-if="chromeEnabled && macTrafficLightPadding"
            class="sidebar-chrome-drag h-full shrink-0 traffic-light-inset"
            aria-hidden="true"
          />
          <!-- 品牌：Windows/Linux 展开状态固定在侧栏左上角；macOS 品牌在顶栏 -->
          <div
            v-if="os !== 'macos'"
            class="sidebar-brand flex h-full min-w-0 flex-1 items-center pl-3.5"
          >
            <div class="inline-flex h-3.5 min-w-0 items-center gap-2">
              <img
                :src="brandIcon"
                :alt="brandName"
                draggable="false"
                class="block h-3.5 w-3.5 shrink-0 select-none rounded-[3px] object-cover grayscale"
              />
              <span class="brand-text flex h-3.5 min-w-0 items-center truncate text-[13px] font-semibold leading-none tracking-wide">{{ brandName }}</span>
            </div>
          </div>
          <!-- macOS: spacer to push the collapse button right -->
          <div v-else class="flex-1" />

          <button
            type="button"
            class="chrome-icon-btn shrink-0"
            :title="t('shell.collapseSidebar')"
            @click="toggleSidebar"
          >
            <PanelLeftClose class="w-4 h-4" />
          </button>
        </WindowDragRegion>

        <div
          v-if="!sidebarCollapsed"
          class="flex flex-1 flex-col min-h-0 min-w-0"
        >
          <!-- C: 项目工作台 -->
          <div class="px-3 py-2 shrink-0 space-y-1">
            <button
              type="button"
              class="sidebar-workbench-link"
              @click="newTask()"
            >
              <Plus class="w-4 h-4" />
              {{ t('shell.newTask') }}
            </button>
            <button
              v-if="isAppAdmin"
              type="button"
              class="sidebar-workbench-link"
              @click="openAutomation"
            >
              <Clock3 class="w-4 h-4" />
              {{ t('shell.scheduledTasks') }}
            </button>
            <button
              v-if="isAppAdmin"
              type="button"
              class="sidebar-workbench-link"
              @click="emit('open-settings', 'skills')"
            >
              <Sparkles class="w-4 h-4" />
              {{ t('shell.skills') }}
            </button>
            <button
              v-if="isAppAdmin"
              type="button"
              class="sidebar-workbench-link"
              @click="emit('open-settings', 'channels')"
            >
              <Link2 class="w-4 h-4" />
              {{ t('shell.connections') }}
            </button>
          </div>

          <section
            v-if="sidebarProjects.length"
            class="sidebar-project-section group/project-section shrink-0 px-2 pb-4"
          >
            <div class="group/section-header mb-1.5 flex h-6 items-center gap-1 px-1">
              <button
                type="button"
                class="sidebar-section-collapse mr-auto"
                :aria-expanded="!projectsSectionCollapsed"
                :title="projectsSectionCollapsed ? t('shell.expandProjectsSection') : t('shell.collapseProjectsSection')"
                @click="toggleProjectsSection"
              >
                <h2 class="sidebar-section-title">{{ t('shell.projects') }}</h2>
                <component
                  :is="projectsSectionCollapsed ? ChevronRight : ChevronDown"
                  class="h-3.5 w-3.5 opacity-0 transition-opacity group-hover/section-header:opacity-100 group-focus-within/section-header:opacity-100"
                />
              </button>
              <div
                class="sidebar-project-search-area relative flex items-center gap-1 opacity-0 transition-opacity group-hover/project-section:opacity-100 group-focus-within/project-section:opacity-100"
              >
                <div
                  class="sidebar-section-search-wrap"
                  :class="projectSearchExpanded && 'is-expanded'"
                >
                  <Search class="sidebar-section-search-icon" aria-hidden="true" />
                  <input
                    v-if="projectSearchExpanded"
                    ref="projectSearchInputRef"
                    v-model="projectSearchQuery"
                    type="search"
                    class="sidebar-section-search-input"
                    :placeholder="t('shell.searchProjects')"
                    :aria-label="t('shell.searchProjects')"
                    @keydown.esc="closeProjectSearch"
                  >
                  <button
                    v-if="projectSearchExpanded && projectSearchQuery"
                    type="button"
                    class="sidebar-section-search-clear"
                    :title="t('common.clearSearch')"
                    :aria-label="t('common.clearSearch')"
                    @click="projectSearchQuery = ''"
                  ><X class="w-3 h-3" /></button>
                </div>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :class="projectSearchExpanded && 'is-active'"
                  :title="projectSearchExpanded ? t('shell.closeProjectSearch') : t('shell.searchProjects')"
                  :aria-expanded="projectSearchExpanded"
                  :aria-label="t('shell.searchProjects')"
                  @click="projectSearchExpanded ? closeProjectSearch() : openProjectSearch()"
                ><Search class="w-3.5 h-3.5" /></button>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :title="t('shell.addProject')"
                  :aria-label="t('shell.addProject')"
                  @click="showProjectCreator = true; projectError = ''"
                ><Plus class="w-3.5 h-3.5" /></button>
              </div>
            </div>
            <Teleport to="body">
              <div
                v-if="showProjectCreator"
                class="fixed inset-0 z-50 flex items-center justify-center bg-foreground/32 p-4"
                @click.self="showProjectCreator = false"
              >
                <section class="project-create-dialog" role="dialog" aria-modal="true" aria-labelledby="project-create-title">
                  <div class="flex items-center justify-between gap-3">
                    <div>
                      <h2 id="project-create-title" class="text-sm font-semibold text-foreground">{{ t('shell.addProject') }}</h2>
                      <p class="mt-0.5 text-[11px] text-muted">{{ t('shell.chooseExistingDirectory') }}</p>
                    </div>
                    <button type="button" class="chrome-icon-btn" :title="t('common.close')" :aria-label="t('common.close')" @click="showProjectCreator = false"><X class="w-4 h-4" /></button>
                  </div>
                  <form class="mt-4" @submit.prevent="addProject">
                    <div class="space-y-3">
                      <label class="block text-xs text-muted">
                        {{ t('shell.projectName') }}
                        <input v-model="projectName" class="project-dialog-input mt-1" :placeholder="t('shell.projectNamePlaceholder')">
                      </label>
                      <div>
                        <label class="block text-xs text-muted">{{ t('shell.projectDirectory') }}</label>
                        <div class="mt-1 flex gap-2">
                          <input v-model="projectRoot" class="project-dialog-input min-w-0 flex-1" :placeholder="t('shell.chooseExistingDirectory')">
                          <button type="button" class="project-dialog-secondary shrink-0" :title="t('shell.chooseExistingDirectory')" @click="pickProjectDirectory"><FolderGit2 class="w-3.5 h-3.5" />{{ t('shell.selectDirectory') }}</button>
                        </div>
                        <div class="mt-2">
                          <SkillDirectoryPicker
                            :title="t('shell.skillsDirectory')"
                            variant="grid"
                            searchable
                            @select="createProjectFromSkill"
                          />
                        </div>
                      </div>
                    </div>
                    <p v-if="projectError" class="mt-3 text-xs text-danger">{{ projectError }}</p>
                    <div class="mt-5 flex justify-end gap-2">
                      <button type="button" class="project-dialog-secondary" @click="showProjectCreator = false">{{ t('common.cancel') }}</button>
                      <button type="submit" class="project-dialog-primary" :disabled="!projectName.trim() || !projectRoot.trim()">{{ t('shell.createProject') }}</button>
                    </div>
                  </form>
                </section>
              </div>
            </Teleport>
            <div
              v-show="!projectsSectionCollapsed"
              class="auto-hide-scrollbar sidebar-section-scroll max-h-[11.75rem] overflow-y-auto"
              @scroll.passive="showScrollbarWhileScrolling"
            >
              <p v-if="projectError" class="mb-1 px-1 text-[11px] text-danger">{{ projectError }}</p>
              <div class="space-y-0.5">
                <div
                  v-for="project in sidebarProjects"
                  :key="project.id"
                  class="relative space-y-1 group/project"
                >
                <div
                  class="sidebar-project-row"
                  :class="chat.current?.projectId === project.id && 'is-active'"
                >
                  <button
                    type="button"
                    class="sidebar-project-expand"
                    :title="isProjectExpanded(project.id) ? t('common.collapse') : t('common.expand')"
                    :aria-expanded="isProjectExpanded(project.id)"
                    :aria-label="isProjectExpanded(project.id) ? t('shell.collapseProject') : t('shell.expandProject')"
                    @click="onProjectChevronClick(project)"
                  >
                    <component
                      :is="isProjectExpanded(project.id) ? ChevronDown : ChevronRight"
                      class="w-3.5 h-3.5 shrink-0"
                    />
                  </button>
                  <button
                    type="button"
                    class="sidebar-project-select"
                    :title="project.workspaceRoot"
                    @click="selectProject(project)"
                  >
                    <FolderGit2 class="w-3.5 h-3.5 shrink-0" />
                    <span class="truncate">{{ displayProjectName(project) }}</span>
                  </button>
                </div>
                <button
                  type="button"
                  class="project-menu-trigger opacity-0 group-hover/project:opacity-100"
                  :title="t('shell.projectActions', { name: displayProjectName(project) })"
                  @click.stop="toggleProjectMenu(project, $event)"
                >
                  <MoreHorizontal class="w-3.5 h-3.5" />
                </button>
                <Teleport to="body">
                  <div
                    v-if="projectMenuId === project.id"
                    class="project-context-menu"
                    :style="{ left: `${projectMenuPosition.left}px`, top: `${projectMenuPosition.top}px` }"
                  >
                    <button type="button" @click="newTask(project); projectMenuId = null"><Plus />{{ t('shell.newLocalTask') }}</button>
                    <button type="button" @click="toggleProjectPin(project)"><Pin />{{ project.isPinned ? t('shell.unpin') : t('shell.pinProject') }}</button>
                    <button type="button" @click="openProjectSettings(project)"><Settings2 />{{ t('shell.projectSettings') }}</button>
                    <button
                      v-if="!project.isDefault"
                      type="button"
                      class="text-danger"
                      @click="requestDeleteProject(project)"
                    ><Trash2 />{{ t('shell.deleteProjectAction') }}</button>
                  </div>
                </Teleport>
                <div
                  v-if="isProjectExpanded(project.id)"
                  class="ml-4 border-l border-border pl-1 space-y-0.5"
                >
                  <div
                    v-for="conversation in projectConversations(project)"
                    :key="conversation.id"
                    class="sidebar-project-task-row group/task flex items-center gap-1 rounded-md"
                    :class="chat.currentId === conversation.id && 'is-active'"
                    @contextmenu="openConversationMenu($event, conversation)"
                  >
                    <div
                      role="button"
                      tabindex="0"
                      class="sidebar-project-conversation flex-1 min-w-0"
                      :title="conversation.title"
                      @click="onRowClick(conversation)"
                      @keydown.enter="onRowClick(conversation)"
                      @keydown.space.prevent="onRowClick(conversation)"
                    >
                      <Loader2
                        v-if="chat.isConversationBusy(conversation.id)"
                        class="w-3.5 h-3.5 shrink-0 animate-spin"
                      />
                      <span
                        v-else-if="chat.isConversationAwaitingView(conversation.id)"
                        class="sidebar-awaiting-dot"
                        :title="t('shell.newActivity')"
                        :aria-label="t('shell.newActivity')"
                      />
                      <MessageSquare v-else class="w-3.5 h-3.5 shrink-0" />
                      <span
                        class="truncate"
                        :title="conversation.title"
                        @dblclick.stop="startEdit(conversation)"
                      >{{ conversation.title }}</span>
                    </div>
                    <template v-if="pendingDeleteId === conversation.id">
                      <button
                        type="button"
                        class="project-task-action"
                        :title="t('common.cancel')"
                        @click.stop="cancelDeleteConversation"
                      ><X /></button>
                      <button
                        type="button"
                        class="project-task-action text-danger"
                        :title="t('shell.confirmDelete')"
                        @click.stop="confirmDeleteConversation(conversation)"
                      ><Check /></button>
                    </template>
                    <template v-else>
                      <button
                        type="button"
                        class="project-task-action opacity-0 group-hover/task:opacity-100"
                        :title="t('shell.pin')"
                        @click.stop="toggleConversationPin(conversation)"
                      ><Pin /></button>
                      <button
                        type="button"
                        class="project-task-action opacity-0 group-hover/task:opacity-100"
                        :title="t('shell.deleteTask')"
                        @click.stop="askDeleteConversation(conversation)"
                      ><Trash2 /></button>
                    </template>
                  </div>
                  <div
                    v-if="!projectLoading.has(project.id) && projectConversations(project).length === 0"
                    class="px-2 py-1 text-[11px] text-muted"
                  >{{ t('shell.noTasks') }}</div>
                  <button
                    v-if="projectCursors[project.id]"
                    type="button"
                    class="sidebar-project-conversation text-accent"
                    :disabled="projectLoading.has(project.id)"
                    @click="loadMoreProjectConversations(project)"
                  >
                    {{ projectLoading.has(project.id) ? t('common.loading') : t('common.loadMore') }}
                  </button>
                </div>
                </div>
              </div>
              <button
                v-if="chat.hasMoreProjects"
                type="button"
                class="sidebar-project-conversation mt-1 text-accent"
                :disabled="chat.loadingMoreProjects"
                @click="loadMoreProjects"
              >{{ chat.loadingMoreProjects ? t('common.loading') : t('shell.loadMoreProjects') }}</button>
            </div>
          </section>

          <section
            v-if="pinnedConversations.length"
            class="sidebar-pinned-section group/pinned-section shrink-0 px-2 pb-4"
          >
            <div class="group/section-header mb-1.5 flex h-6 items-center px-1">
              <button
                type="button"
                class="sidebar-section-collapse mr-auto"
                :aria-expanded="!pinnedSectionCollapsed"
                :title="pinnedSectionCollapsed ? t('shell.expandPinnedSection') : t('shell.collapsePinnedSection')"
                @click="togglePinnedSection"
              >
                <h2 class="sidebar-section-title">{{ t('shell.pinned') }}</h2>
                <component
                  :is="pinnedSectionCollapsed ? ChevronRight : ChevronDown"
                  class="h-3.5 w-3.5 opacity-0 transition-opacity group-hover/section-header:opacity-100 group-focus-within/section-header:opacity-100"
                />
              </button>
            </div>
            <div
              v-show="!pinnedSectionCollapsed"
              class="auto-hide-scrollbar sidebar-section-scroll max-h-[11.75rem] overflow-y-auto"
              @scroll.passive="showScrollbarWhileScrolling"
            >
              <div class="space-y-0.5">
                <div
                  v-for="c in pinnedConversations"
                  :key="c.id"
                  class="sidebar-conv-row group relative flex h-9 items-center gap-2 px-3 rounded-lg cursor-pointer transition-colors"
                  :class="chat.currentId === c.id
                    ? 'is-selected bg-foreground/10'
                    : 'hover:bg-hover'"
                  @click="onRowClick(c)"
                  @contextmenu="openConversationMenu($event, c)"
                >
                  <Loader2
                    v-if="chat.isConversationBusy(c.id)"
                    class="w-3.5 h-3.5 shrink-0 animate-spin"
                    :class="chat.currentId === c.id ? 'text-foreground' : 'text-muted'"
                  />
                  <span
                    v-else-if="chat.isConversationAwaitingView(c.id)"
                    class="sidebar-awaiting-dot"
                    :title="t('shell.newActivity')"
                    :aria-label="t('shell.newActivity')"
                  />
                  <MessageSquare
                    v-else
                    class="w-3.5 h-3.5 shrink-0"
                    :class="chat.currentId === c.id ? 'text-foreground' : 'text-muted'"
                  />
                  <div
                    class="flex-1 min-w-0 text-[13px] text-foreground truncate"
                    :title="c.title"
                    @dblclick.stop="startEdit(c)"
                  >{{ c.title }}</div>
                  <div
                    class="sidebar-row-actions"
                    :class="pendingDeleteId === c.id && 'is-pending'"
                  >
                    <div class="sidebar-row-actions-fade" aria-hidden="true" />
                    <div class="sidebar-row-actions-btns">
                    <template v-if="pendingDeleteId === c.id">
                      <button
                        type="button"
                        class="p-1 rounded hover:bg-hover cursor-pointer"
                        :title="t('common.cancel')"
                        @click.stop="cancelDeleteConversation()"
                      >
                        <X class="w-3.5 h-3.5 text-muted" />
                      </button>
                      <button
                        type="button"
                        class="p-1 rounded hover:bg-danger/15 cursor-pointer"
                        :title="t('shell.confirmDelete')"
                        @click.stop="confirmDeleteConversation(c)"
                      >
                        <Check class="w-3.5 h-3.5 text-danger" />
                      </button>
                    </template>
                    <template v-else>
                      <button
                        type="button"
                        class="p-1 rounded hover:bg-hover cursor-pointer"
                        :title="t('shell.unpin')"
                        @click.stop="toggleConversationPin(c)"
                      >
                        <PinOff class="w-3.5 h-3.5 text-muted" />
                      </button>
                      <button
                        type="button"
                        class="p-1 rounded hover:bg-hover cursor-pointer"
                        :title="t('common.delete')"
                        @click.stop="askDeleteConversation(c)"
                      >
                      <Trash2 class="w-3.5 h-3.5 text-muted" />
                    </button>
                    </template>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </section>

          <!-- D: 最近对话（标题固定；仅列表滚动） -->
          <div
            class="sidebar-conversation-section group/conversation-section flex min-h-0 flex-1 flex-col px-2 pb-3"
          >
            <div class="group/section-header mb-1.5 flex h-6 shrink-0 items-center gap-1 px-1">
              <button
                type="button"
                class="sidebar-section-collapse mr-auto"
                :aria-expanded="!conversationsSectionCollapsed"
                :title="conversationsSectionCollapsed ? t('shell.expandRecentSection') : t('shell.collapseRecentSection')"
                @click="toggleConversationsSection"
              >
                <h2 class="sidebar-section-title">{{ t('shell.recent') }}</h2>
                <component
                  :is="conversationsSectionCollapsed ? ChevronRight : ChevronDown"
                  class="h-3.5 w-3.5 opacity-0 transition-opacity group-hover/section-header:opacity-100 group-focus-within/section-header:opacity-100"
                />
              </button>
              <div
                class="sidebar-conversation-search-area relative flex items-center gap-1 opacity-0 transition-opacity group-hover/conversation-section:opacity-100 group-focus-within/conversation-section:opacity-100"
              >
                <div
                  class="sidebar-section-search-wrap"
                  :class="conversationSearchExpanded && 'is-expanded'"
                >
                  <Search class="sidebar-section-search-icon" aria-hidden="true" />
                  <input
                    v-if="conversationSearchExpanded"
                    ref="conversationSearchInputRef"
                    v-model="searchQuery"
                    type="search"
                    class="sidebar-section-search-input"
                    :placeholder="t('shell.searchSessions')"
                    :aria-label="t('shell.searchSessions')"
                    @keydown.esc="closeConversationSearch"
                  >
                  <button
                    v-if="conversationSearchExpanded && searchQuery"
                    type="button"
                    class="sidebar-section-search-clear"
                    :title="t('common.clearSearch')"
                    :aria-label="t('common.clearSearch')"
                    @click="searchQuery = ''"
                  ><X class="w-3 h-3" /></button>
                </div>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :class="conversationSearchExpanded && 'is-active'"
                  :title="conversationSearchExpanded ? t('shell.closeSessionSearch') : t('shell.searchSessions')"
                  :aria-expanded="conversationSearchExpanded"
                  :aria-label="t('shell.searchSessions')"
                  @click="conversationSearchExpanded ? closeConversationSearch() : openConversationSearch()"
                ><Search class="w-3.5 h-3.5" /></button>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :title="t('shell.newTask')"
                  :aria-label="t('shell.newTask')"
                  @click="newTask()"
                ><Plus class="w-3.5 h-3.5" /></button>
              </div>
            </div>
            <div
              ref="listScroller"
              class="auto-hide-scrollbar sidebar-section-scroll min-h-0 flex-1 overflow-y-auto"
              style="overflow-anchor: none"
              @scroll.passive="showScrollbarWhileScrolling"
            >
            <div v-show="!conversationsSectionCollapsed">
            <div
              v-for="c in sidebarRows"
              :key="c.id"
            >
            <div
              class="sidebar-conv-row group relative flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition-colors"
              :class="chat.currentId === c.id
                ? 'is-selected bg-foreground/10'
                : 'hover:bg-hover'"
              @click="onRowClick(c)"
              @contextmenu="openConversationMenu($event, c)"
            >
              <Loader2
                v-if="chat.isConversationBusy(c.id)"
                class="w-3.5 h-3.5 shrink-0 animate-spin"
                :class="chat.currentId === c.id ? 'text-foreground' : 'text-muted'"
              />
              <span
                v-else-if="chat.isConversationAwaitingView(c.id)"
                class="sidebar-awaiting-dot"
                :title="t('shell.newActivity')"
                :aria-label="t('shell.newActivity')"
              />
              <MessageSquare
                v-else
                class="w-3.5 h-3.5 shrink-0"
                :class="chat.currentId === c.id ? 'text-foreground' : 'text-muted'"
              />
              <div class="flex-1 min-w-0">
                <div class="flex min-w-0 items-center gap-1.5">
                  <div
                    class="min-w-0 flex-1 truncate text-[13px] text-foreground"
                    :title="c.title"
                    @dblclick.stop="startEdit(c)"
                  >{{ c.title }}</div>
                  <i18n-t
                    v-if="(c.matchCount ?? 0) > 1"
                    keypath="shell.matchCount"
                    tag="button"
                    type="button"
                    class="sidebar-search-match-count"
                    :title="t('shell.matchCount', { n: c.matchCount })"
                    :aria-expanded="expandedSearchId === c.id"
                    @click.stop="toggleSearchMatches(c.id)"
                  >
                    <template #n><span>{{ c.matchCount }}</span></template>
                  </i18n-t>
                </div>
                <div
                  v-if="c.snippet"
                  class="text-[10px] text-muted truncate"
                  :title="c.snippet"
                >{{ c.snippet }}</div>
              </div>
              <div
                v-if="!searchQuery.trim()"
                class="sidebar-row-actions"
                :class="pendingDeleteId === c.id && 'is-pending'"
              >
                <div class="sidebar-row-actions-fade" aria-hidden="true" />
                <div class="sidebar-row-actions-btns">
                <template v-if="pendingDeleteId === c.id">
                  <button
                    class="p-1 rounded hover:bg-hover cursor-pointer"
                    @click.stop="cancelDeleteConversation()"
                    :title="t('common.cancel')"
                  >
                    <X class="w-3.5 h-3.5 text-muted" />
                  </button>
                  <button
                    class="p-1 rounded hover:bg-danger/15 cursor-pointer"
                    @click.stop="confirmDeleteConversation(c)"
                    :title="t('shell.confirmDelete')"
                  >
                    <Check class="w-3.5 h-3.5 text-danger" />
                  </button>
                </template>
                <template v-else>
                  <button
                    class="p-1 rounded hover:bg-hover cursor-pointer"
                    @click.stop="toggleConversationPin(c)"
                    :title="t('shell.pin')"
                  >
                    <Pin class="w-3.5 h-3.5 text-muted" />
                  </button>
                  <button
                    class="p-1 rounded hover:bg-hover cursor-pointer"
                    @click.stop="askDeleteConversation(c)"
                    :title="t('common.delete')"
                  >
                    <Trash2 class="w-3.5 h-3.5 text-muted" />
                  </button>
                </template>
                </div>
              </div>
            </div>
            <div
              v-if="(c.matches?.length ?? 0) > 0"
              class="sidebar-search-matches"
              :class="expandedSearchId === c.id && 'is-open'"
              :aria-hidden="expandedSearchId !== c.id"
            >
              <div class="sidebar-search-matches-inner">
                <div
                  class="sidebar-search-matches-list auto-hide-scrollbar"
                  @scroll.passive="showScrollbarWhileScrolling"
                >
                  <button
                    v-for="m in c.matches"
                    :key="m.messageId"
                    type="button"
                    class="sidebar-search-match-item"
                    :title="m.snippet"
                    :tabindex="expandedSearchId === c.id ? 0 : -1"
                    @click.stop="onMatchClick(c, m, $event)"
                  >{{ matchRoleLabel(m.role) }} · {{ m.snippet }}</button>
                </div>
              </div>
            </div>
            </div>
            <!-- Sentinel for infinite scroll; observed by IntersectionObserver -->
            <div ref="sentinel" v-if="!searchQuery.trim()" class="h-1 w-full" />
            <div
              v-if="searchLoading"
              class="px-3 py-2 text-center text-xs text-muted"
            >
              {{ t('shell.searching') }}
            </div>
            <div
              v-else-if="chat.loadingMoreConversations"
              class="px-3 py-2 text-center text-xs text-muted"
            >
              {{ t('common.loading') }}
            </div>
            <div v-if="!searchLoading && !sidebarRows.length" class="px-3 py-8 text-center text-xs text-muted">
              {{ searchQuery.trim() ? t('shell.noMatchingSessions') : t('shell.noSessions') }}
            </div>
            </div>
            </div>
          </div>

          <!-- F: 侧栏底栏 -->
          <div class="p-2 border-t border-border flex shrink-0 items-center gap-1">
            <DesktopSnapshotButton v-if="desktopSnapshotEnabled" />
            <AccountMenu @open-settings="section => $emit('open-settings', section)" />
          </div>
        </div>
      </aside>

      <div class="chat-main shell-chat flex-1 min-w-0 flex flex-col">
        <!-- D0: 对话区顶栏：macOS 品牌常驻顶栏；Windows/Linux 品牌在侧栏，收缩时顶栏补位 -->
        <ChatTopBar
          :collapsed="sidebarCollapsed"
          :traffic-light-padding="chromeEnabled && macTrafficLightPadding"
          :show-brand="chromeEnabled && (os === 'macos' || sidebarCollapsed)"
          :show-project-picker="chromeEnabled"
          @expand-sidebar="toggleSidebar"
          @new-task="newTask"
        >
          <template #actions>
            <button
              v-if="isAppAdmin && !workspacePanelOpen"
              type="button"
              class="chrome-icon-btn"
              :title="t('shell.openWorkspace')"
              :aria-label="t('shell.openWorkspace')"
              @click="setWorkspacePanelOpen(true)"
            >
              <PanelRightOpen class="w-4 h-4" />
            </button>
            <WindowControls
              v-if="useMainAreaWindowControls"
              class="window-controls-win"
              :maximized="maximized"
              @minimize="minimize"
              @maximize="toggleMaximize"
              @close="closeWindow"
            />
          </template>
        </ChatTopBar>

        <!-- E: 聊天正文 -->
        <WindowDragRegion region="chat-body" as="main" class="flex-1 min-h-0 flex flex-col relative">
          <slot />
        </WindowDragRegion>
      </div>

      <WorkspacePanel
        v-if="isAppAdmin && workspacePanelOpen"
        :workspace-root="chat.current?.workspaceRoot ?? ''"
        :conversation-id="chat.current?.id ?? ''"
        @initialize-git="chat.sendUserMessage(GIT_INITIALIZATION_TASK)"
        @install-git="chat.sendUserMessage(t('shell.installGitPrompt'))"
        @close="setWorkspacePanelOpen(false)"
      />
    </div>
    <Teleport to="body">
      <div
        v-if="conversationMenu"
        class="fixed inset-0 z-[300]"
        @mousedown="closeConversationMenu"
        @contextmenu.prevent="closeConversationMenu"
      >
        <div
          class="conversation-context-menu"
          role="menu"
          :style="{ left: `${conversationMenu.left}px`, top: `${conversationMenu.top}px` }"
          @mousedown.stop
        >
          <button
            type="button"
            role="menuitem"
            @click="onConversationMenuPin"
          >
            <PinOff v-if="conversationMenu.target.isPinned" />
            <Pin v-else />
            {{ conversationMenu.target.isPinned ? t('shell.unpin') : t('shell.pin') }}
          </button>
          <button type="button" role="menuitem" @click="onConversationMenuRename">
            <Pencil />{{ t('common.rename') }}
          </button>
          <div class="conversation-context-separator" />
          <button type="button" role="menuitem" @click="onConversationMenuCopyId">
            <Copy />{{ t('shell.copySessionId') }}
          </button>
          <button
            type="button"
            role="menuitem"
            :disabled="!conversationMenu.target.workspaceRoot"
            @click="onConversationMenuCopyWorkspace"
          >
            <Copy />{{ t('shell.copyWorkspace') }}
          </button>
          <button
            v-if="isTauriRuntime()"
            type="button"
            role="menuitem"
            :disabled="!conversationMenu.target.workspaceRoot"
            @click="onConversationMenuRevealWorkspace"
          >
            <FolderOpen />{{ t('shell.showInFinder') }}
          </button>
        </div>
      </div>
    </Teleport>
    <div
      v-if="renameTarget"
      class="fixed inset-0 z-50 flex items-center justify-center bg-foreground/32 p-4"
      @click.self="cancelRename"
    >
      <section
        class="w-full max-w-sm rounded-xl border border-border bg-card p-4 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="rename-conversation-title"
      >
        <div class="mb-3 flex items-center justify-between">
          <h2 id="rename-conversation-title" class="text-sm font-semibold text-foreground">{{ t('shell.renameSession') }}</h2>
          <button type="button" class="chrome-icon-btn" :title="t('common.close')" :aria-label="t('common.close')" @click="cancelRename">
            <X class="w-4 h-4" />
          </button>
        </div>
        <label class="block text-xs text-muted">
          {{ t('shell.sessionName') }}
          <input
            ref="renameInputRef"
            v-model="renameTitle"
            type="text"
            class="mt-1 w-full rounded border border-border bg-background px-2 py-1.5 text-sm text-foreground outline-none focus:border-accent"
            @keydown.enter="onRenameEnter"
            @keydown="onRenameKeydown"
            @compositionstart="renameComposing = true"
            @compositionend="onRenameCompositionEnd"
          >
        </label>
        <div class="mt-5 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover"
            @click="cancelRename"
          >{{ t('common.cancel') }}</button>
          <button
            type="button"
            class="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-foreground disabled:opacity-50"
            :disabled="!renameTitle.trim()"
            @click="saveRename"
          >{{ t('common.save') }}</button>
        </div>
      </section>
    </div>
    <div
      v-if="projectEditing"
      class="fixed inset-0 z-50 flex items-center justify-center bg-foreground/32 p-4"
      @click.self="projectEditing = null"
    >
      <section class="w-full max-w-md rounded-xl border border-border bg-card p-4 shadow-xl">
        <div class="mb-3 flex items-center justify-between">
          <h2 class="text-sm font-semibold text-foreground">{{ t('shell.projectSettings') }}</h2>
          <button class="chrome-icon-btn" @click="projectEditing = null"><X class="w-4 h-4" /></button>
        </div>
        <div class="space-y-3">
          <label class="block text-xs text-muted">
            {{ t('shell.projectName') }}
            <input v-model="projectEditing.name" class="mt-1 w-full rounded border border-border bg-background px-2 py-1.5 text-sm text-foreground">
          </label>
          <label class="block text-xs text-muted">
            {{ t('shell.projectDirectory') }}
            <input v-model="projectEditing.workspaceRoot" class="mt-1 w-full rounded border border-border bg-background px-2 py-1.5 text-sm text-foreground">
          </label>
        </div>
        <p v-if="projectError" class="mt-2 text-xs text-danger">{{ projectError }}</p>
        <div class="mt-5 flex justify-end gap-2">
          <button class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover" @click="projectEditing = null">{{ t('common.cancel') }}</button>
          <button
            class="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-foreground"
            :disabled="!projectEditing.name.trim() || !projectEditing.workspaceRoot.trim()"
            @click="persistProject(projectEditing); projectEditing = null"
          >{{ t('common.save') }}</button>
        </div>
      </section>
    </div>
    <div
      v-if="projectPendingDeletion"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-[hsl(var(--foreground)/0.32)] p-4"
      @click.self="projectPendingDeletion = null"
    >
      <section class="w-full max-w-sm rounded-xl border border-border bg-card p-5">
        <h2 class="text-base font-semibold text-foreground">{{ t('shell.deleteProject') }}</h2>
        <p class="mt-2 text-sm leading-6 text-muted">
          {{ t('shell.deleteProjectConfirm', { name: displayProjectName(projectPendingDeletion) }) }}
        </p>
        <div class="mt-3 rounded-lg border border-border bg-hover/50 px-3 py-2">
          <div class="text-[11px] font-medium text-muted">{{ t('shell.projectDirectory') }}</div>
          <div class="mt-0.5 break-all font-mono text-xs text-foreground">
            {{ projectPendingDeletion.workspaceRoot || t('shell.directoryNotSet') }}
          </div>
        </div>
        <div class="mt-5 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover"
            @click="projectPendingDeletion = null"
          >{{ t('common.cancel') }}</button>
          <button
            type="button"
            class="rounded-md bg-danger px-3 py-1.5 text-sm text-white hover:opacity-90"
            @click="confirmProjectDeletion"
          >{{ t('shell.deleteProjectAction') }}</button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
/* Flush scrollbars to the sidebar right edge; keep row content inset via section px-*. */
.sidebar-section-scroll {
  margin-right: -0.5rem; /* match section px-2 */
  padding-right: 0.5rem;
}

/* Conversation row: fade title into the action cluster; keep icons sharp. */
.sidebar-conv-row {
  --sidebar-row-fill: hsl(var(--shell-sidebar));
}
.sidebar-conv-row:hover {
  --sidebar-row-fill: hsl(var(--hover));
}
.sidebar-conv-row.is-selected,
.sidebar-conv-row.is-selected:hover {
  --sidebar-row-fill: color-mix(
    in srgb,
    hsl(var(--foreground)) 10%,
    hsl(var(--shell-sidebar))
  );
}
.sidebar-row-actions {
  position: absolute;
  inset-block: 0;
  right: 0;
  z-index: 1;
  display: flex;
  align-items: stretch;
  padding-right: 0.5rem;
  border-radius: inherit;
  opacity: 0;
  pointer-events: none;
  transition: opacity 150ms;
}
.sidebar-conv-row:hover .sidebar-row-actions,
.sidebar-row-actions.is-pending {
  opacity: 1;
  pointer-events: auto;
}
.sidebar-row-actions-fade {
  width: 1.25rem;
  pointer-events: none;
  background: linear-gradient(to right, transparent, var(--sidebar-row-fill));
}
.sidebar-row-actions-btns {
  display: flex;
  align-items: center;
  gap: 0.125rem;
  background: var(--sidebar-row-fill);
}

.sidebar-search-match-count {
  flex: 0 0 auto;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 10px;
  line-height: 1.25rem;
  color: hsl(var(--muted));
  white-space: nowrap;
  cursor: pointer;
}
.sidebar-search-match-count span {
  color: hsl(var(--accent));
}

.sidebar-search-matches {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows 180ms ease;
}
.sidebar-search-matches.is-open {
  grid-template-rows: 1fr;
}
.sidebar-search-matches:not(.is-open) {
  pointer-events: none;
}
.sidebar-search-matches-inner {
  min-height: 0;
  overflow: hidden;
}
.sidebar-search-matches-list {
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  margin: 0 0.5rem 0.25rem 2rem;
  max-height: 12rem;
  overflow-x: hidden;
  overflow-y: auto;
}
.sidebar-search-match-item {
  flex: 0 0 auto;
  min-height: 1.5rem;
  padding: 0.25rem 0.375rem;
  border: 0;
  border-radius: 0.25rem;
  background: transparent;
  color: hsl(var(--muted));
  font-size: 10px;
  line-height: 1.4;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.sidebar-search-match-item:hover {
  background: hsl(var(--hover));
}

.sidebar-workbench-link {
  @apply w-full h-8 px-3 rounded-lg text-[13px] text-foreground inline-flex items-center gap-2 text-left hover:bg-hover transition-colors;
}

.sidebar-workbench-link :deep(svg) {
  @apply text-muted;
}

.sidebar-section-title {
  @apply text-[11px] leading-5 font-medium text-muted;
}

.sidebar-section-collapse {
  @apply inline-flex h-6 min-w-0 items-center gap-1 rounded-md text-muted transition-colors hover:text-foreground;
}

.sidebar-section-header-action {
  @apply h-6 w-6 shrink-0 rounded-md inline-flex items-center justify-center text-muted hover:bg-hover hover:text-foreground transition;
}

.sidebar-section-header-action.is-active {
  @apply bg-hover text-foreground;
}

.sidebar-section-search-wrap {
  @apply absolute right-0 top-0 h-6 w-0 overflow-hidden rounded-md border border-transparent bg-card opacity-0 transition-[width,opacity,border-color] duration-200;
  direction: ltr;
}

.sidebar-section-search-wrap.is-expanded {
  @apply w-36 border-border opacity-100;
}

.sidebar-section-search-icon {
  @apply pointer-events-none absolute left-1.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted;
}

.sidebar-section-search-input {
  @apply h-full w-full appearance-none bg-transparent py-0 pl-7 pr-6 text-xs leading-6 text-foreground outline-none placeholder:text-muted;
  transform: translateY(-3px);
}

.sidebar-section-search-clear {
  @apply absolute right-1 top-1/2 h-4 w-4 -translate-y-1/2 rounded text-muted hover:bg-hover hover:text-foreground;
}

.project-create-dialog {
  @apply w-full max-w-md rounded-xl border border-border bg-card p-5 shadow-2xl;
}

.project-dialog-input {
  @apply w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground outline-none placeholder:text-muted focus:border-accent focus:ring-1 focus:ring-accent/30;
}

.project-dialog-secondary {
  @apply inline-flex items-center justify-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-xs text-foreground transition hover:bg-hover disabled:cursor-not-allowed disabled:opacity-50;
}

.project-dialog-primary {
  @apply rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-accent-foreground transition hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50;
}

.sidebar-project-input {
  @apply w-full rounded-md border border-border bg-background px-2 py-1 text-xs text-foreground outline-none placeholder:text-muted focus:border-accent;
}

.sidebar-project-create {
  @apply w-full rounded-md bg-accent px-2 py-1 text-xs text-accent-foreground disabled:cursor-not-allowed disabled:opacity-50;
}

.sidebar-project-row {
  @apply w-full h-9 pl-1 pr-8 rounded-lg inline-flex items-center gap-0.5 text-[13px] text-muted text-left hover:bg-hover hover:text-foreground transition-colors;
}

.sidebar-project-row.is-active,
.sidebar-project-row.is-active:hover {
  @apply bg-foreground/10 text-foreground;
}

.sidebar-project-expand {
  @apply h-7 w-7 shrink-0 rounded-md inline-flex items-center justify-center text-muted hover:bg-hover hover:text-foreground transition-colors cursor-pointer;
}

.sidebar-project-select {
  @apply min-w-0 flex-1 h-9 pr-1 rounded-md inline-flex items-center gap-2 text-[13px] text-inherit text-left cursor-pointer;
}

.sidebar-project-conversation {
  @apply h-7 px-2 inline-flex items-center gap-2 text-[12px] text-muted text-left hover:text-foreground transition-colors;
}

button.sidebar-project-conversation {
  @apply w-full rounded-md hover:bg-hover;
}

.sidebar-project-task-row .sidebar-project-conversation {
  @apply rounded-none bg-transparent hover:bg-transparent;
}

.sidebar-project-task-row:hover {
  @apply bg-hover;
}

.sidebar-project-task-row.is-active,
.sidebar-project-task-row.is-active:hover {
  @apply bg-foreground/10 text-foreground;
}

.sidebar-project-task-row.is-active .sidebar-project-conversation {
  @apply text-foreground;
}

.project-task-action {
  @apply h-6 w-6 shrink-0 rounded inline-flex items-center justify-center text-muted hover:bg-hover hover:text-foreground transition;
}

.project-task-action :deep(svg) {
  @apply w-3 h-3;
}

.project-context-menu {
  @apply fixed z-[300] w-40 rounded-lg border border-border bg-card p-1 shadow-xl;
}

.project-menu-trigger {
  @apply absolute right-1 top-1 h-6 w-6 rounded-md inline-flex items-center justify-center text-muted hover:bg-hover hover:text-foreground transition;
}

.project-context-menu button {
  @apply w-full flex items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground hover:bg-hover;
}

.project-context-menu button :deep(svg) {
  @apply w-3.5 h-3.5 text-muted;
}

.conversation-context-menu {
  @apply fixed z-[301] w-48 rounded-lg border border-border bg-card p-1 shadow-xl select-none;
}

.conversation-context-menu button {
  @apply w-full flex items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground hover:bg-hover disabled:opacity-40 disabled:pointer-events-none;
}

.conversation-context-menu button :deep(svg) {
  @apply w-3.5 h-3.5 text-muted;
}

.conversation-context-separator {
  @apply my-1 h-px bg-border;
}

.sidebar-awaiting-dot {
  @apply inline-block h-2 w-2 shrink-0 rounded-full bg-accent;
  animation: sidebar-awaiting-pulse 1.6s ease-in-out infinite;
}

@keyframes sidebar-awaiting-pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.35;
    transform: scale(0.85);
  }
}

@media (prefers-reduced-motion: reduce) {
  .sidebar-awaiting-dot {
    animation: none;
  }
  .sidebar-search-matches {
    transition: none;
  }
}
</style>
