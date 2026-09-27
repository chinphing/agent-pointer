<script setup lang="ts">
import { resolveBrandName } from '../../lib/webBranding'

const brandName = resolveBrandName()
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Check, FolderOpen, FolderPlus, PanelLeftOpen, Plus } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { isTauriRuntime } from '../../lib/runtime'
import { createProject } from '../../lib/api'
import { applyProjectCreationResult, projectNameFromWorkspaceRoot } from '../../lib/projectCreation'
import WindowDragRegion from '../layout/WindowDragRegion.vue'
import SkillDirectoryPicker from '../skills/SkillDirectoryPicker.vue'

const props = withDefaults(
  defineProps<{
    /** Whether the sidebar is collapsed: when true, the "expand sidebar / new task" buttons merge in next to the brand, keeping the top row single-line. */
    collapsed?: boolean
    /** Whether to reserve top-left space for macOS traffic-light overlay buttons (applies once the collapsed sidebar disappears). */
    trafficLightPadding?: boolean
    /** Whether to show the brand text (Pointer ·): on desktop (Tauri) the top bar shows it when the sidebar is collapsed; when the sidebar is expanded the brand lives in its top-left corner and is hidden here. */
    showBrand?: boolean
    /** Whether to show the project picker button: desktop (Tauri) keeps it pinned to the top bar; on web, project switching lives in the sidebar and is not shown here. */
    showProjectPicker?: boolean
  }>(),
  { collapsed: false, trafficLightPadding: false, showBrand: true, showProjectPicker: true }
)

const emit = defineEmits<{
  (e: 'expand-sidebar'): void
  (e: 'new-task'): void
}>()

function isEphemeralWorkspacePath(path: string): boolean {
  const normalized = path.replace(/\\/g, '/')
  return normalized.includes('/session-sandboxes/') || normalized.includes('/coder-sandboxes/')
}

const { t } = useI18n()
const chat = useChatStore()

const projectLocked = computed(
  () => !!chat.current?.projectId || (chat.current?.messages.length ?? 0) > 0
)
const selectedProject = computed(() =>
  chat.projectById(chat.current?.projectId ?? chat.current?.pendingProjectId)
)
const projectPickerOpen = ref(false)
const projectPickerButtonRef = ref<HTMLButtonElement | null>(null)
const projectPickerRef = ref<HTMLElement | null>(null)
const projectCreationPending = ref(false)
const projectDropdownDirection = ref<'up' | 'down'>('up')
const projectDropdownMaxHeight = ref<number | null>(null)

/** The project picker holds a lot of content; pick a direction and cap height by available space above/below the button to avoid overflowing the viewport. */
function updateProjectDropdownPlacement() {
  const btn = projectPickerButtonRef.value
  if (!btn) return
  const rect = btn.getBoundingClientRect()
  const viewportHeight = window.innerHeight
  const spaceAbove = rect.top
  const spaceBelow = viewportHeight - rect.bottom
  const preferUp = spaceAbove >= spaceBelow
  projectDropdownDirection.value = preferUp ? 'up' : 'down'
  const available = Math.max(120, preferUp ? spaceAbove : spaceBelow)
  projectDropdownMaxHeight.value = Math.min(available - 8, 416)
}

watch(projectPickerOpen, open => {
  if (open) {
    void nextTick(updateProjectDropdownPlacement)
  }
})

const workspaceTooltip = computed(() => {
  const p = chat.current?.workspaceRoot?.trim()
  if (!p) return t('chat.topBar.workspaceHintEmpty')
  if (isEphemeralWorkspacePath(p)) return t('chat.topBar.workspaceHintTemp', { path: p })
  return p
})

const workspaceLabel = computed(() =>
  selectedProject.value
    ? selectedProject.value.isDefault
      ? t('chat.topBar.defaultProject')
      : selectedProject.value.name
    : t('chat.topBar.selectProject')
)

function selectProject(projectId: string) {
  if (projectLocked.value) return
  const currentProjectId = chat.current?.projectId ?? chat.current?.pendingProjectId
  // Clicking the already-selected folder clears the selection without closing the dropdown.
  if (projectId === currentProjectId) {
    clearWorkspace()
    return
  }
  if (chat.setConversationProject(projectId)) projectPickerOpen.value = false
}

async function createOrSelectWorkspaceProject(workspaceRoot: string): Promise<boolean> {
  const root = workspaceRoot.trim()
  if (!root || projectCreationPending.value || projectLocked.value) return false
  projectCreationPending.value = true
  try {
    const result = await createProject(projectNameFromWorkspaceRoot(root), root)
    await applyProjectCreationResult(result, {
      refreshProjects: chat.refreshProjects,
      selectProject: async projectId => {
        if (!chat.setConversationProject(projectId)) {
          throw new Error('conversation project selection is locked')
        }
      },
      notify: message => chat.showUiToast(message, 'warning')
    })
    projectPickerOpen.value = false
    return true
  } catch (error) {
    console.error('[chat-topbar] create project from workspace failed', { workspaceRoot: root, error })
    chat.showUiToast(t('chat.toast.projectCreateFailed'), 'error')
    return false
  } finally {
    projectCreationPending.value = false
  }
}

async function onSkillDirectorySelect(dir: { name: string; path: string }) {
  const ok = await createOrSelectWorkspaceProject(dir.path)
  // Skill-directory projects default to the coder agent (skill scripts / code engineering tasks).
  if (ok && !projectLocked.value) {
    chat.setConversationAgent('coder', 'single')
  }
}

async function pickWorkspaceFolder() {
  if (!isTauriRuntime()) return
  const conv = chat.current || chat.newConversation()
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const current = conv.workspaceRoot?.trim()
    const dir = await open({
      directory: true,
      multiple: false,
      ...(current ? { defaultPath: current } : {})
    })
    if (typeof dir === 'string' && dir) {
      await createOrSelectWorkspaceProject(dir)
    }
  } catch (e) {
    console.error('[chat-topbar] pick workspace folder failed', e)
    chat.showUiToast(t('chat.toast.dirPickFailed'), 'error')
  }
}

const workspaceComposing = ref(false)

async function commitWorkspaceInput() {
  await createOrSelectWorkspaceProject(chat.current?.workspaceRoot ?? '')
}

/** Enter commits the workspace path — but never while an IME is composing (Chinese candidate confirm). */
function onWorkspaceEnter(event: KeyboardEvent) {
  if (event.isComposing || workspaceComposing.value) return
  event.preventDefault()
  void commitWorkspaceInput()
}

/** Guard the post-compositionend window where the confirming Enter still arrives. */
function onWorkspaceCompositionEnd() {
  setTimeout(() => {
    workspaceComposing.value = false
  }, 50)
}

function onWorkspaceInputChange() {
  chat.setConversationWorkspace(chat.current?.workspaceRoot ?? '')
}

function clearWorkspace() {
  chat.setConversationWorkspace('')
}

function onWorkspaceInput(e: Event) {
  const conv = chat.current || chat.newConversation()
  conv.workspaceRoot = (e.target as HTMLInputElement).value
  onWorkspaceInputChange()
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (projectPickerOpen.value && projectPickerButtonRef.value && projectPickerRef.value) {
    if (!projectPickerButtonRef.value.contains(target) && !projectPickerRef.value.contains(target)) {
      projectPickerOpen.value = false
    }
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <WindowDragRegion
    region="main-top-chrome"
    class="chat-topbar shell-chat flex h-10 shrink-0 items-center gap-1.5 px-4"
  >
    <div
      v-if="collapsed"
      class="hidden md:flex shrink-0 items-center gap-1 mr-0.5"
      :class="trafficLightPadding ? 'traffic-light-inset' : ''"
    >
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="展开侧栏"
        aria-label="展开侧栏"
        @click="emit('expand-sidebar')"
      >
        <PanelLeftOpen class="w-4 h-4" />
      </button>
      <button
        type="button"
        class="chrome-icon-btn shrink-0"
        title="新建任务"
        aria-label="新建任务"
        @click="emit('new-task')"
      >
        <Plus class="w-4 h-4" />
      </button>
    </div>
    <template v-if="showBrand || showProjectPicker">
      <span
        v-if="showBrand"
        class="brand-text text-[13px] leading-none whitespace-nowrap shrink-0"
      >{{ brandName }}</span>
      <span
        v-if="showBrand"
        class="text-[11px] leading-none text-muted shrink-0"
      >·</span>
      <div
        v-if="showProjectPicker"
        class="relative flex min-w-0 items-center"
      >
        <button
          ref="projectPickerButtonRef"
          type="button"
          class="composer-agent-trigger chat-topbar-project-btn max-w-[240px]"
          :class="projectLocked ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'"
          :title="projectLocked ? '项目已锁定' : workspaceTooltip"
          :disabled="projectLocked"
          @click="projectPickerOpen = !projectPickerOpen"
        >
          <FolderOpen class="chat-topbar-project-icon w-3.5 h-3.5 shrink-0 text-muted" />
          <span class="truncate max-w-[180px] text-foreground">{{ workspaceLabel }}</span>
        </button>
        <div
          v-if="projectPickerOpen && !projectLocked"
          ref="projectPickerRef"
          class="composer-dropdown composer-project-dropdown flex flex-col"
          :class="projectDropdownDirection === 'down' ? 'composer-dropdown--down' : 'composer-dropdown--up'"
          :style="projectDropdownMaxHeight != null ? { maxHeight: `${projectDropdownMaxHeight}px` } : undefined"
        >
          <div
            class="p-1.5"
            :class="projectDropdownDirection === 'down' ? 'order-0 border-b border-border' : 'order-last border-t border-border'"
          >
            <button
              v-if="isTauriRuntime()"
              type="button"
              class="composer-dropdown-item composer-dropdown-item--compact cursor-pointer"
              :disabled="projectCreationPending"
              @click="pickWorkspaceFolder"
            >
              <FolderPlus class="w-3.5 h-3.5 shrink-0" />
              <span class="whitespace-nowrap text-foreground">
                {{ projectCreationPending ? '正在创建项目…' : '本地目录' }}
              </span>
            </button>
            <input
              v-else
              ref="workspaceInputRef"
              :value="chat.current?.workspaceRoot ?? ''"
              type="text"
              placeholder="输入本地目录创建新项目"
              aria-label="输入本地目录创建新项目"
              class="composer-workspace-input"
              :title="workspaceTooltip"
              @input="onWorkspaceInput"
              @keydown.enter="onWorkspaceEnter"
              @compositionstart="workspaceComposing = true"
              @compositionend="onWorkspaceCompositionEnd"
            />
          </div>
          <div class="px-3 pb-1 pt-2">
            <div class="text-[10px] text-muted font-medium whitespace-nowrap">已有项目</div>
          </div>
          <div class="max-h-44 space-y-0.5 overflow-y-auto p-1">
            <button
              v-for="project in chat.projects.filter(p => !p.isArchived)"
              :key="project.id"
              type="button"
              class="composer-dropdown-item composer-dropdown-item--compact cursor-pointer"
              :class="project.id === (chat.current?.projectId ?? chat.current?.pendingProjectId) ? 'composer-dropdown-item-active' : ''"
              @click="selectProject(project.id)"
            >
              <FolderOpen class="w-3 h-3 shrink-0" />
              <span class="flex-1 truncate">{{ project.isDefault ? '默认项目' : project.name }}</span>
              <Check
                v-if="project.id === (chat.current?.projectId ?? chat.current?.pendingProjectId)"
                class="h-3 w-3 shrink-0 text-muted"
              />
            </button>
          </div>
          <div class="border-t border-border p-1.5">
            <SkillDirectoryPicker
              title="技能目录"
              variant="list"
              :disabled="projectCreationPending"
              @select="onSkillDirectorySelect"
            />
          </div>
        </div>
      </div>
    </template>
    <div class="ml-auto hidden md:flex shrink-0 items-center">
      <slot name="actions" />
    </div>
  </WindowDragRegion>
</template>

<style scoped>
.composer-project-dropdown {
  width: min(200px, calc(100vw - 2rem));
}
</style>
