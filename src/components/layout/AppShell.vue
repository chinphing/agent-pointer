<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import {
  Pencil,
  Plus,
  Search,
  Settings,
  MessageSquare,
  Loader2,
  Trash2,
  Check,
  X,
  Bot,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightOpen,
  FolderGit2,
  Clock3,
  Sparkles,
  Link2,
  ChevronDown,
  ChevronRight,
  MoreHorizontal,
  Pin,
  Settings2
} from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { useWorkspacePanelStore } from '../../stores/workspacePanel'
import {
  createProject, searchConversations, updateProject
} from '../../lib/api'
import { GIT_INITIALIZATION_TASK } from '../../lib/workspacePanel'
import { applyProjectCreationResult } from '../../lib/projectCreation'
import { useWindowChrome } from '../../composables/useWindowChrome'
import { useSidebarCollapse } from '../../composables/useSidebarCollapse'
import WindowControls from './WindowControls.vue'
import WindowDragRegion from './WindowDragRegion.vue'
import DesktopSnapshotButton from './DesktopSnapshotButton.vue'
import { isTauriRuntime } from '../../lib/runtime'
import WorkspacePanel from '../workspace/WorkspacePanel.vue'
import type { Project } from '../../types/chat'

const emit = defineEmits<{
  (e: 'open-settings', section?: string): void
  (e: 'open-automation'): void
  (e: 'open-skills'): void
}>()

const chat = useChatStore()
const workspacePanel = useWorkspacePanelStore()
const { collapsed: sidebarCollapsed, toggle: toggleSidebar } = useSidebarCollapse()
const workspacePanelOpen = computed(() => workspacePanel.open)

function setWorkspacePanelOpen(open: boolean) {
  workspacePanel.setOpen(open)
}

/**
 * Two-step inline delete confirmation. The first click on the trash icon
 * reveals inline 确认/取消 buttons; only the second click (确认) actually
 * deletes. We avoid `window.confirm` because Tauri's webview does not render
 * native browser dialogs — `window.confirm` returns true without any UI,
 * making a naive guard a no-op on desktop.
 */
const pendingDeleteId = ref<string | null>(null)

/** Inline edit (rename) state for a sidebar conversation title. */
const editingId = ref<string | null>(null)
const editingTitle = ref('')
const editInputRef = ref<HTMLInputElement | null>(null)

function startEdit(conv: { id: string; title: string }) {
  saveEdit()
  editingId.value = conv.id
  editingTitle.value = conv.title
  pendingDeleteId.value = null
  nextTick(() => {
    editInputRef.value?.focus()
    editInputRef.value?.select()
  })
}

function saveEdit() {
  if (!editingId.value) return
  const newTitle = editingTitle.value.trim()
  if (newTitle) {
    chat.renameConversation(editingId.value, newTitle)
  }
  editingId.value = null
  editingTitle.value = ''
}

function cancelEdit() {
  editingId.value = null
  editingTitle.value = ''
}

/** ESC exits edit without saving. */
function onEditKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    cancelEdit()
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

/**
 * Pointer-down optimistic highlight so the sidebar paints selection before the
 * click handler runs heavy open/hydrate work (long transcripts).
 */
const optimisticConversationId = ref<string | null>(null)

function isConversationHighlighted(id: string): boolean {
  return id === (optimisticConversationId.value ?? chat.currentId)
}

function onConversationPointerDown(id: string) {
  const key = id.trim()
  if (!key) return
  optimisticConversationId.value = key
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
  saveEdit()
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
  // openConversation sets currentId synchronously; drop optimistic once store matches.
  if (chat.currentId === c.id) {
    optimisticConversationId.value = null
  }
}

watch(
  () => chat.currentId,
  id => {
    if (optimisticConversationId.value != null && optimisticConversationId.value === id) {
      optimisticConversationId.value = null
    }
  }
)

const projectSearchQuery = ref('')
const projectSearchExpanded = ref(false)
const projectSearchInputRef = ref<HTMLInputElement | null>(null)
const conversationSearchExpanded = ref(false)
const conversationSearchInputRef = ref<HTMLInputElement | null>(null)
const projectsSectionCollapsed = ref(false)
const conversationsSectionCollapsed = ref(false)
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
const expandedProjectIds = ref(new Set<string>())
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
  return project.isDefault ? '默认项目' : project.name
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
    projectError.value = '当前环境不支持目录选择，请手动填写目录路径'
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
    projectError.value = '请输入项目名称并选择项目目录'
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
    .filter(conversation => conversation.projectId === project.id)
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

async function toggleProject(project: Project) {
  const expanded = new Set(expandedProjectIds.value)
  if (expanded.has(project.id)) {
    expanded.delete(project.id)
  } else {
    expanded.add(project.id)
    if (!projectCursors.value[project.id] && projectConversations(project).length === 0) {
      await loadMoreProjectConversations(project)
    }
  }
  expandedProjectIds.value = expanded
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

function openSkills() {
  emit('open-skills')
}

function openProjectSearch() {
  closeConversationSearch()
  projectsSectionCollapsed.value = false
  projectSearchExpanded.value = true
  nextTick(() => projectSearchInputRef.value?.focus())
}

function closeProjectSearch() {
  projectSearchExpanded.value = false
  projectSearchQuery.value = ''
}

function openConversationSearch() {
  closeProjectSearch()
  conversationsSectionCollapsed.value = false
  conversationSearchExpanded.value = true
  nextTick(() => conversationSearchInputRef.value?.focus())
}

function closeConversationSearch() {
  conversationSearchExpanded.value = false
  searchQuery.value = ''
}

function toggleProjectsSection() {
  projectsSectionCollapsed.value = !projectsSectionCollapsed.value
  if (projectsSectionCollapsed.value) closeProjectSearch()
}

function toggleConversationsSection() {
  conversationsSectionCollapsed.value = !conversationsSectionCollapsed.value
  if (conversationsSectionCollapsed.value) closeConversationSearch()
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
}

function closeProjectOverlaysOnKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
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

/** Windows / Linux: min/max/close on main top-right (or collapsed top strip). */
const useMainAreaWindowControls = computed(
  () => showCustomControls.value && (os.value === 'windows' || os.value === 'linux')
)
const windowControlsOnCollapsedTop = computed(
  () => useMainAreaWindowControls.value && sidebarCollapsed.value
)
const windowControlsOnMainTop = computed(
  () => useMainAreaWindowControls.value && !sidebarCollapsed.value
)

const searchQuery = ref('')
const searchLoading = ref(false)
type SidebarRow = {
  id: string
  title: string
  updatedAt: number
  snippet?: string
  messageId?: string
  messageCount?: number
  projectId?: string
}
const searchResults = ref<SidebarRow[]>([])
let searchTimer: ReturnType<typeof setTimeout> | null = null
let searchSeq = 0

function visibleConversations(list: typeof chat.conversations) {
  const activeId = chat.currentId
  return list.filter(c => {
    const isolated = c.id.startsWith('cron:') || c.id.startsWith('webhook:')
    return !isolated || c.id === activeId
  })
}

const sidebarRows = computed((): SidebarRow[] => {
  if (searchQuery.value.trim()) return searchResults.value
  return visibleConversations(chat.conversations).map(c => ({
    id: c.id,
    title: c.title,
    updatedAt: c.updatedAt
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
        projectId: h.projectId
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
const scrollbarHideTimers = new Map<HTMLElement, ReturnType<typeof setTimeout>>()

function showScrollbarWhileScrolling(event: Event) {
  const target = event.currentTarget
  if (!(target instanceof HTMLElement)) return
  target.classList.add('is-scrolling')
  const existingTimer = scrollbarHideTimers.get(target)
  if (existingTimer) clearTimeout(existingTimer)
  scrollbarHideTimers.set(target, setTimeout(() => {
    target.classList.remove('is-scrolling')
    scrollbarHideTimers.delete(target)
  }, 600))
}

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
  for (const timer of scrollbarHideTimers.values()) clearTimeout(timer)
  scrollbarHideTimers.clear()
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

// Dismiss the inline delete confirmation and editing state when the active
// conversation or search filter changes, so a stale pending state never lingers.
watch(() => chat.currentId, () => {
  pendingDeleteId.value = null
  editingId.value = null
  editingTitle.value = ''
})
// Dismiss editing when sidebar collapses.
watch(sidebarCollapsed, (collapsed) => {
  if (collapsed) {
    editingId.value = null
    editingTitle.value = ''
  }
})
watch(searchQuery, q => {
  pendingDeleteId.value = null
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
    <!-- H: 收起全宽顶栏 -->
    <WindowDragRegion
      v-if="sidebarCollapsed"
      region="collapsed-top-chrome"
      class="collapsed-top-chrome hidden md:flex shrink-0 items-center gap-1 pr-2 select-none bg-transparent"
      :class="chromeEnabled && macTrafficLightPadding
        ? 'traffic-light-inset mac-chrome-row'
        : 'h-10 pl-2'"
    >
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="展开侧栏"
        @click="toggleSidebar"
      >
        <PanelLeftOpen class="w-4 h-4" />
      </button>
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="新建任务"
        @click="newTask()"
      >
        <Plus class="w-4 h-4" />
      </button>
      <div
        v-if="chromeEnabled"
        class="flex-1 min-w-0 h-full"
      />
      <WindowControls
        v-if="windowControlsOnCollapsedTop"
        class="window-controls-win"
        :maximized="maximized"
        @minimize="minimize"
        @maximize="toggleMaximize"
        @close="closeWindow"
      />
    </WindowDragRegion>

    <div class="flex flex-1 min-h-0">
      <aside
        class="app-sidebar hidden md:flex shrink-0 flex-col border-r border-border bg-card transition-[width] duration-200 ease-out overflow-hidden"
        :class="sidebarCollapsed ? 'w-0 border-r-0' : 'w-[260px]'"
      >
        <!-- A: 侧栏顶栏 -->
        <WindowDragRegion
          v-if="!sidebarCollapsed"
          region="sidebar-top-chrome"
          class="sidebar-chrome shrink-0 flex items-center gap-1 pr-2 select-none bg-transparent"
          :class="chromeEnabled && macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
        >
          <div
            v-if="chromeEnabled"
            class="sidebar-chrome-drag flex-1 min-w-0 h-full"
            :class="macTrafficLightPadding ? 'traffic-light-inset' : 'pl-2'"
          />
          <div
            v-else
            class="flex items-center gap-2 min-w-0 flex-1 pl-2"
          >
            <div class="w-7 h-7 rounded-lg bg-accent/15 border border-border flex items-center justify-center shrink-0">
              <Bot class="w-3.5 h-3.5 text-accent" />
            </div>
            <div class="text-[13px] font-semibold tracking-wide brand-text truncate">Pointer</div>
          </div>

          <button
            type="button"
            class="chrome-icon-btn shrink-0"
            title="收起侧栏"
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
              新建任务
            </button>
            <button
              type="button"
              class="sidebar-workbench-link"
              @click="openAutomation"
            >
              <Clock3 class="w-4 h-4" />
              定时任务
            </button>
            <button
              type="button"
              class="sidebar-workbench-link"
              @click="openSkills"
            >
              <Sparkles class="w-4 h-4" />
              技能
            </button>
            <button
              type="button"
              class="sidebar-workbench-link"
              @click="emit('open-settings', 'channels')"
            >
              <Link2 class="w-4 h-4" />
              连接
            </button>
          </div>

          <section
            v-if="sidebarProjects.length"
            class="sidebar-project-section group/project-section shrink-0 px-3 pb-4"
          >
            <div class="group/section-header mb-2 flex h-6 items-center gap-1">
              <button
                type="button"
                class="sidebar-section-collapse mr-auto"
                :aria-expanded="!projectsSectionCollapsed"
                :title="projectsSectionCollapsed ? '展开项目' : '收起项目'"
                @click="toggleProjectsSection"
              >
                <h2 class="sidebar-section-title">项目</h2>
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
                    placeholder="搜索项目"
                    aria-label="搜索项目"
                    @keydown.esc="closeProjectSearch"
                  >
                  <button
                    v-if="projectSearchExpanded && projectSearchQuery"
                    type="button"
                    class="sidebar-section-search-clear"
                    title="清除搜索"
                    aria-label="清除搜索"
                    @click="projectSearchQuery = ''"
                  ><X class="w-3 h-3" /></button>
                </div>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :class="projectSearchExpanded && 'is-active'"
                  :title="projectSearchExpanded ? '关闭项目搜索' : '搜索项目'"
                  :aria-expanded="projectSearchExpanded"
                  aria-label="搜索项目"
                  @click="projectSearchExpanded ? closeProjectSearch() : openProjectSearch()"
                ><Search class="w-3.5 h-3.5" /></button>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  title="新增项目"
                  aria-label="新增项目"
                  @click="showProjectCreator = true; projectError = ''"
                ><Plus class="w-3.5 h-3.5" /></button>
              </div>
            </div>
            <Teleport to="body">
              <div
                v-if="showProjectCreator"
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
                @click.self="showProjectCreator = false"
              >
                <section class="project-create-dialog" role="dialog" aria-modal="true" aria-labelledby="project-create-title">
                  <div class="flex items-center justify-between gap-3">
                    <div>
                      <h2 id="project-create-title" class="text-sm font-semibold text-foreground">新增项目</h2>
                      <p class="mt-0.5 text-[11px] text-muted">选择已有目录</p>
                    </div>
                    <button type="button" class="chrome-icon-btn" title="关闭" aria-label="关闭" @click="showProjectCreator = false"><X class="w-4 h-4" /></button>
                  </div>
                  <form class="mt-4" @submit.prevent="addProject">
                    <div class="space-y-3">
                      <label class="block text-xs text-muted">
                        项目名称
                        <input v-model="projectName" class="project-dialog-input mt-1" placeholder="例如：Pointer App">
                      </label>
                      <div>
                        <label class="block text-xs text-muted">项目目录</label>
                        <div class="mt-1 flex gap-2">
                          <input v-model="projectRoot" class="project-dialog-input min-w-0 flex-1" placeholder="选择已有目录">
                          <button type="button" class="project-dialog-secondary shrink-0" title="选择已有目录" @click="pickProjectDirectory"><FolderGit2 class="w-3.5 h-3.5" />选择目录</button>
                        </div>
                      </div>
                    </div>
                    <p v-if="projectError" class="mt-3 text-xs text-danger">{{ projectError }}</p>
                    <div class="mt-5 flex justify-end gap-2">
                      <button type="button" class="project-dialog-secondary" @click="showProjectCreator = false">取消</button>
                      <button type="submit" class="project-dialog-primary" :disabled="!projectName.trim() || !projectRoot.trim()">创建项目</button>
                    </div>
                  </form>
                </section>
              </div>
            </Teleport>
            <div
              v-show="!projectsSectionCollapsed"
              class="sidebar-auto-scrollbar -mr-3 max-h-[11.75rem] overflow-y-auto pr-3"
              @scroll.passive="showScrollbarWhileScrolling"
            >
              <p v-if="projectError" class="mb-1 px-1 text-[11px] text-danger">{{ projectError }}</p>
              <div class="space-y-0.5">
                <div
                  v-for="project in sidebarProjects"
                  :key="project.id"
                  class="relative space-y-1 group/project"
                >
                <button
                  type="button"
                  class="sidebar-project-row"
                  :class="chat.current?.projectId === project.id && 'is-active'"
                  :title="project.workspaceRoot"
                  :aria-expanded="expandedProjectIds.has(project.id)"
                  @click="toggleProject(project)"
                >
                  <component
                    :is="expandedProjectIds.has(project.id) ? ChevronDown : ChevronRight"
                    class="w-3.5 h-3.5 shrink-0"
                  />
                  <FolderGit2 class="w-3.5 h-3.5 shrink-0" />
                  <span class="truncate">{{ displayProjectName(project) }}</span>
                </button>
                <button
                  type="button"
                  class="project-menu-trigger opacity-0 group-hover/project:opacity-100"
                  :title="`${displayProjectName(project)} 操作`"
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
                    <button type="button" @click="newTask(project); projectMenuId = null"><Plus />新建本地任务</button>
                    <button type="button" @click="toggleProjectPin(project)"><Pin />{{ project.isPinned ? '取消置顶' : '置顶项目' }}</button>
                    <button type="button" @click="openProjectSettings(project)"><Settings2 />项目设置</button>
                    <button
                      v-if="!project.isDefault"
                      type="button"
                      class="text-danger"
                      @click="requestDeleteProject(project)"
                    ><Trash2 />删除项目</button>
                  </div>
                </Teleport>
                <div
                  v-if="expandedProjectIds.has(project.id)"
                  class="ml-4 border-l border-border pl-1 space-y-0.5"
                >
                  <div
                    v-for="conversation in projectConversations(project)"
                    :key="conversation.id"
                    class="sidebar-project-task-row group/task flex items-center gap-1 rounded-md"
                    :class="isConversationHighlighted(conversation.id) && 'is-active'"
                  >
                    <div
                      role="button"
                      tabindex="0"
                      class="sidebar-project-conversation flex-1 min-w-0"
                      :class="isConversationHighlighted(conversation.id) && 'is-active'"
                      :title="conversation.title"
                      @pointerdown.left="onConversationPointerDown(conversation.id)"
                      @click="onRowClick(conversation)"
                      @keydown.enter="onRowClick(conversation)"
                      @keydown.space.prevent="onRowClick(conversation)"
                    >
                      <Loader2
                        v-if="chat.isConversationGenerating(conversation.id)"
                        class="w-3.5 h-3.5 shrink-0 animate-spin"
                      />
                      <MessageSquare v-else class="w-3.5 h-3.5 shrink-0" />
                      <input
                        v-if="editingId === conversation.id"
                        ref="editInputRef"
                        v-model="editingTitle"
                        type="text"
                        class="w-full bg-transparent border border-accent rounded px-1 text-[12px] text-foreground outline-none"
                        @click.stop
                        @keydown.enter.prevent="saveEdit"
                        @keydown="onEditKeydown"
                        @blur="saveEdit"
                      />
                      <span v-else class="truncate">{{ conversation.title }}</span>
                    </div>
                    <template v-if="pendingDeleteId === conversation.id">
                      <button
                        type="button"
                        class="project-task-action"
                        title="取消"
                        @click.stop="cancelDeleteConversation"
                      ><X /></button>
                      <button
                        type="button"
                        class="project-task-action text-danger"
                        title="确认删除"
                        @click.stop="confirmDeleteConversation(conversation)"
                      ><Check /></button>
                    </template>
                    <template v-else-if="editingId !== conversation.id">
                      <button
                        type="button"
                        class="project-task-action opacity-0 group-hover/task:opacity-100"
                        title="重命名任务"
                        @click.stop="startEdit(conversation)"
                      ><Pencil /></button>
                      <button
                        type="button"
                        class="project-task-action opacity-0 group-hover/task:opacity-100"
                        title="删除任务"
                        @click.stop="askDeleteConversation(conversation)"
                      ><Trash2 /></button>
                    </template>
                  </div>
                  <div
                    v-if="!projectLoading.has(project.id) && projectConversations(project).length === 0"
                    class="px-2 py-1 text-[11px] text-muted"
                  >无任务</div>
                  <button
                    v-if="projectCursors[project.id]"
                    type="button"
                    class="sidebar-project-conversation text-accent"
                    :disabled="projectLoading.has(project.id)"
                    @click="loadMoreProjectConversations(project)"
                  >
                    {{ projectLoading.has(project.id) ? '加载中…' : '加载更多' }}
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
              >{{ chat.loadingMoreProjects ? '加载中…' : '加载更多项目' }}</button>
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
                :title="conversationsSectionCollapsed ? '展开最近' : '收起最近'"
                @click="toggleConversationsSection"
              >
                <h2 class="sidebar-section-title">最近</h2>
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
                    placeholder="搜索会话"
                    aria-label="搜索会话"
                    @keydown.esc="closeConversationSearch"
                  >
                  <button
                    v-if="conversationSearchExpanded && searchQuery"
                    type="button"
                    class="sidebar-section-search-clear"
                    title="清除搜索"
                    aria-label="清除搜索"
                    @click="searchQuery = ''"
                  ><X class="w-3 h-3" /></button>
                </div>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  :class="conversationSearchExpanded && 'is-active'"
                  :title="conversationSearchExpanded ? '关闭会话搜索' : '搜索会话'"
                  :aria-expanded="conversationSearchExpanded"
                  aria-label="搜索会话"
                  @click="conversationSearchExpanded ? closeConversationSearch() : openConversationSearch()"
                ><Search class="w-3.5 h-3.5" /></button>
                <button
                  type="button"
                  class="sidebar-section-header-action"
                  title="新建任务"
                  aria-label="新建任务"
                  @click="newTask()"
                ><Plus class="w-3.5 h-3.5" /></button>
              </div>
            </div>
            <div
              ref="listScroller"
              class="sidebar-auto-scrollbar min-h-0 flex-1 overflow-y-auto"
              style="overflow-anchor: none"
              @mousedown.self="saveEdit"
              @scroll.passive="showScrollbarWhileScrolling"
            >
            <div v-show="!conversationsSectionCollapsed">
            <div
              v-for="c in sidebarRows"
              :key="c.id"
              class="group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer transition border"
              :class="isConversationHighlighted(c.id)
                ? 'bg-accent-muted border-accent/40'
                : 'hover:bg-hover border-transparent'"
              @pointerdown.left="onConversationPointerDown(c.id)"
              @click="onRowClick(c)"
            >
              <Loader2
                v-if="chat.isConversationGenerating(c.id)"
                class="w-3.5 h-3.5 shrink-0 animate-spin"
                :class="isConversationHighlighted(c.id) ? 'text-accent' : 'text-muted'"
              />
              <MessageSquare
                v-else
                class="w-3.5 h-3.5 shrink-0"
                :class="isConversationHighlighted(c.id) ? 'text-accent' : 'text-muted'"
              />
              <div class="flex-1 min-w-0">
                <template v-if="editingId === c.id">
                  <input
                    ref="editInputRef"
                    v-model="editingTitle"
                    type="text"
                    class="w-full bg-transparent border border-accent rounded px-1 text-[13px] text-foreground outline-none"
                    @click.stop
                    @keydown.enter.prevent="saveEdit"
                    @keydown="onEditKeydown"
                    @blur="saveEdit"
                  />
                </template>
                <template v-else>
                  <div
                    class="text-[13px] text-foreground truncate cursor-text"
                    :title="c.title"
                    @dblclick.stop="startEdit(c)"
                  >{{ c.title }}</div>
                </template>
                <div
                  v-if="c.snippet"
                  class="text-[10px] text-muted truncate"
                  :title="c.snippet"
                >{{ c.snippet }}</div>
                <div v-else class="text-[10px] text-muted">{{ new Date(c.updatedAt).toLocaleString() }}</div>
              </div>
              <template v-if="pendingDeleteId === c.id">
                <button
                  class="p-1 rounded hover:bg-hover cursor-pointer"
                  @click.stop="cancelDeleteConversation()"
                  title="取消"
                >
                  <X class="w-3.5 h-3.5 text-muted" />
                </button>
                <button
                  class="p-1 rounded hover:bg-danger/15 cursor-pointer"
                  @click.stop="confirmDeleteConversation(c)"
                  title="确认删除"
                >
                  <Check class="w-3.5 h-3.5 text-danger" />
                </button>
              </template>
              <template v-else-if="editingId !== c.id">
                <button
                  class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-hover cursor-pointer"
                  @click.stop="startEdit(c)"
                  title="重命名"
                >
                  <Pencil class="w-3.5 h-3.5 text-muted" />
                </button>
                <button
                  class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-hover cursor-pointer"
                  @click.stop="askDeleteConversation(c)"
                  title="删除"
                >
                  <Trash2 class="w-3.5 h-3.5 text-muted" />
                </button>
              </template>
            </div>
            <!-- Sentinel for infinite scroll; observed by IntersectionObserver -->
            <div ref="sentinel" v-if="!searchQuery.trim()" class="h-1 w-full" />
            <div
              v-if="searchLoading"
              class="px-3 py-2 text-center text-xs text-muted"
            >
              搜索中…
            </div>
            <div
              v-else-if="chat.loadingMoreConversations"
              class="px-3 py-2 text-center text-xs text-muted"
            >
              加载中…
            </div>
            <div v-if="!searchLoading && !sidebarRows.length" class="px-3 py-8 text-center text-xs text-muted">
              {{ searchQuery.trim() ? '没有找到匹配的会话' : '没有会话' }}
            </div>
            </div>
            </div>
          </div>

          <!-- F: 侧栏底栏 -->
          <div class="p-2 border-t border-border flex shrink-0 items-center gap-1">
            <DesktopSnapshotButton v-if="!isTauriRuntime()" />
            <button
              class="chrome-icon-btn"
              title="设置"
              @click="$emit('open-settings')"
            >
              <Settings class="w-4 h-4" />
            </button>
          </div>
        </div>
      </aside>

      <div class="flex-1 min-w-0 flex flex-col">
        <!-- D: 主区顶栏 -->
        <WindowDragRegion
          v-if="chromeEnabled && !sidebarCollapsed"
          region="main-top-chrome"
          class="main-top-chrome hidden md:flex shrink-0 items-stretch select-none min-h-10"
          :class="macTrafficLightPadding ? 'mac-chrome-row' : 'h-10'"
        >
          <div class="main-chrome-drag flex-1 min-w-0 h-full min-h-10" />
          <button
            v-if="!workspacePanelOpen"
            type="button"
            class="chrome-icon-btn self-center mr-1"
            title="打开工作区"
            @click="setWorkspacePanelOpen(true)"
          >
            <PanelRightOpen class="w-4 h-4" />
          </button>
          <WindowControls
            v-if="windowControlsOnMainTop"
            class="window-controls-win"
            :maximized="maximized"
            @minimize="minimize"
            @maximize="toggleMaximize"
            @close="closeWindow"
          />
        </WindowDragRegion>

        <!-- E: 聊天正文 -->
        <WindowDragRegion region="chat-body" as="main" class="flex-1 min-h-0 flex flex-col relative">
          <button
            v-if="!workspacePanelOpen && (!chromeEnabled || sidebarCollapsed)"
            type="button"
            class="hidden lg:flex absolute right-2 top-2 z-10 chrome-icon-btn"
            title="打开工作区"
            @click="setWorkspacePanelOpen(true)"
          >
            <PanelRightOpen class="w-4 h-4" />
          </button>
          <slot />
        </WindowDragRegion>
      </div>

      <WorkspacePanel
        v-if="workspacePanelOpen"
        :workspace-root="chat.current?.workspaceRoot ?? ''"
        :conversation-id="chat.current?.id ?? ''"
        @initialize-git="chat.sendUserMessage(GIT_INITIALIZATION_TASK)"
        @install-git="chat.sendUserMessage('帮我安装 Git')"
        @close="setWorkspacePanelOpen(false)"
      />
    </div>
    <div
      v-if="projectEditing"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="projectEditing = null"
    >
      <section class="w-full max-w-md rounded-xl border border-border bg-card p-4 shadow-xl">
        <div class="mb-3 flex items-center justify-between">
          <h2 class="text-sm font-semibold text-foreground">项目设置</h2>
          <button class="chrome-icon-btn" @click="projectEditing = null"><X class="w-4 h-4" /></button>
        </div>
        <div class="space-y-3">
          <label class="block text-xs text-muted">
            项目名称
            <input v-model="projectEditing.name" class="mt-1 w-full rounded border border-border bg-background px-2 py-1.5 text-sm text-foreground">
          </label>
          <label class="block text-xs text-muted">
            项目目录
            <input v-model="projectEditing.workspaceRoot" class="mt-1 w-full rounded border border-border bg-background px-2 py-1.5 text-sm text-foreground">
          </label>
        </div>
        <p v-if="projectError" class="mt-2 text-xs text-danger">{{ projectError }}</p>
        <div class="mt-5 flex justify-end gap-2">
          <button class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover" @click="projectEditing = null">取消</button>
          <button
            class="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-foreground"
            :disabled="!projectEditing.name.trim() || !projectEditing.workspaceRoot.trim()"
            @click="persistProject(projectEditing); projectEditing = null"
          >保存</button>
        </div>
      </section>
    </div>
    <div
      v-if="projectPendingDeletion"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-[hsl(var(--foreground)/0.32)] p-4"
      @click.self="projectPendingDeletion = null"
    >
      <section class="w-full max-w-sm rounded-xl border border-border bg-card p-5">
        <h2 class="text-base font-semibold text-foreground">删除项目？</h2>
        <p class="mt-2 text-sm leading-6 text-muted">
          删除“{{ displayProjectName(projectPendingDeletion) }}”会删除项目下所有会话记录，
          不会删除本地文件。确认删除项目吗？
        </p>
        <div class="mt-3 rounded-lg border border-border bg-hover/50 px-3 py-2">
          <div class="text-[11px] font-medium text-muted">项目目录</div>
          <div class="mt-0.5 break-all font-mono text-xs text-foreground">
            {{ projectPendingDeletion.workspaceRoot || '未设置目录' }}
          </div>
        </div>
        <div class="mt-5 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-border px-3 py-1.5 text-sm text-foreground hover:bg-hover"
            @click="projectPendingDeletion = null"
          >取消</button>
          <button
            type="button"
            class="rounded-md bg-danger px-3 py-1.5 text-sm text-white hover:opacity-90"
            @click="confirmProjectDeletion"
          >删除项目</button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.sidebar-auto-scrollbar {
  scrollbar-color: transparent transparent;
  scrollbar-width: thin;
}

.sidebar-auto-scrollbar::-webkit-scrollbar-thumb {
  background: transparent;
  transition: background-color 150ms ease;
}

.sidebar-auto-scrollbar.is-scrolling {
  scrollbar-color: hsl(var(--border)) transparent;
}

.sidebar-auto-scrollbar.is-scrolling::-webkit-scrollbar-thumb {
  background: hsl(var(--border));
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
  @apply bg-accent-muted text-accent;
}

.sidebar-section-search-wrap {
  @apply absolute right-0 top-0 h-6 w-0 overflow-hidden rounded-md border border-transparent bg-background opacity-0 transition-[width,opacity,border-color] duration-200;
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
  @apply rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white transition hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50;
}

.sidebar-project-input {
  @apply w-full rounded-md border border-border bg-background px-2 py-1 text-xs text-foreground outline-none placeholder:text-muted focus:border-accent;
}

.sidebar-project-create {
  @apply w-full rounded-md bg-accent px-2 py-1 text-xs text-white disabled:cursor-not-allowed disabled:opacity-50;
}

.sidebar-project-row {
  @apply w-full h-9 pl-2 pr-8 rounded-lg inline-flex items-center gap-2 text-[13px] text-muted text-left hover:bg-hover hover:text-foreground transition-colors;
}

.sidebar-project-row.is-active {
  @apply bg-accent-muted text-accent;
}

.sidebar-project-conversation {
  @apply w-full h-7 px-2 rounded-md inline-flex items-center gap-2 text-[12px] text-muted text-left hover:bg-hover hover:text-foreground transition-colors;
}

.sidebar-project-conversation.is-active {
  @apply bg-accent-muted text-accent;
}

.sidebar-project-task-row:hover {
  @apply bg-hover;
}

.sidebar-project-task-row.is-active,
.sidebar-project-task-row.is-active:hover {
  @apply bg-accent-muted;
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
</style>
