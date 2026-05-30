<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { ChevronDown, FolderOpen, Send, Square, X } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { resolveAgentUi, resolveLeadAgentUi, composerAgentLabel } from '../../lib/agentUi'
import { iconForAgent, sortComposerAgents, TEAM_MODE_UI_ENABLED } from '../../lib/agentIcons'
import type { AgentDef, ComputerMonitor } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../../types/chat'
import {
  listAgents,
  listComputerMonitors,
  setComputerConversationMonitor
} from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'
import ComputerScreenPickerModal from './ComputerScreenPickerModal.vue'
import WorkspaceRequiredModal from './WorkspaceRequiredModal.vue'

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()

const tokenQuotaBlocked = computed(() => platformAuth.tokenQuotaExhausted)
const needsPlatformLogin = computed(() => isTauriRuntime() && !platformAuth.session.logged_in)
const showLoginBanner = computed(
  () => needsPlatformLogin.value && (chat.current?.messages.length ?? 0) > 0
)
const composerPlaceholder = computed(() => {
  if (needsPlatformLogin.value) {
    return '请先登录 Pointer 账户'
  }
  if (tokenQuotaBlocked.value) {
    return '套餐 Token 额度已用尽，请前往官网充值'
  }
  return settings.settings.hasKey ? '与 Pointer 对话…' : '请先在设置中配置 API Key'
})

const text = ref('')
const composing = ref(false)
const showAgentPicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const agentBtnRef = ref<HTMLButtonElement | null>(null)
const agentPickerRef = ref<HTMLDivElement | null>(null)
const agentPickerWidth = ref(0)
const workspaceInputRef = ref<HTMLInputElement | null>(null)

const agents = ref<AgentDef[]>([])

const leadUi = computed(() => resolveLeadAgentUi(settings.settings, agents.value))

const workers = computed(() => {
  const list = agents.value.filter(a => {
    if (a.role !== 'worker' || !a.enabled) return false
    return resolveAgentUi(a, settings.settings).userSelectable
  })
  return sortComposerAgents(list)
})

const selectedWorker = computed(() => {
  if (settings.settings.agentMode !== 'single') return undefined
  const id = settings.settings.leadAgentId?.trim()
  if (!id) return workers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
  return workers.value.find(w => w.id === id)
})

const selectedWorkerId = computed(() => selectedWorker.value?.id?.trim() || DEFAULT_LEAD_AGENT_ID)

function isLeadAgentSelected(agentId: string): boolean {
  const st = settings.settings
  if (st.agentMode !== 'single') return false
  const id = st.leadAgentId?.trim() || DEFAULT_LEAD_AGENT_ID
  return id === agentId
}

const showComputerMonitorPicker = computed(
  () => settings.settings.agentMode === 'single' && leadUi.value.showComputerMonitorPicker
)

const needsWorkspace = computed(
  () =>
    settings.settings.agentMode === 'single'
    && selectedWorkerId.value === 'coder'
    && leadUi.value.showWorkspacePicker
)

const supervisorRoundsLabel = computed(() => {
  if (settings.settings.agentMode !== 'supervisor' || !chat.current) return ''
  const used = chat.current.toolRoundsUsedSupervisor ?? 0
  const max = settings.settings.maxSubAgentToolRounds ?? settings.settings.maxToolRounds ?? 100
  return `子任务轮次 ${used}/${max}`
})

const workspaceDirName = computed(() => {
  const p = chat.current?.workspaceRoot ?? ''
  if (!p) return ''
  return p.replace(/[/\\]+$/, '').split(/[/\\]/).pop() || ''
})

const workspaceTooltip = computed(() => {
  const p = chat.current?.workspaceRoot?.trim()
  return p || '未设置工作目录'
})

const currentAgentLabel = computed(() => composerAgentLabel(selectedWorker.value, settings.settings))

const currentAgentIcon = computed(() => iconForAgent(selectedWorker.value, settings.settings))

const hasWorkspace = computed(() => !!(chat.current?.workspaceRoot?.trim()))

const canSend = computed(
  () =>
    text.value.trim().length > 0 &&
    !chat.generating &&
    !needsPlatformLogin.value &&
    !tokenQuotaBlocked.value &&
    settings.settings.hasKey
)

async function onPlatformLogin() {
  try {
    await platformAuth.login()
  } catch {
    /* error in store */
  }
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
}

async function loadAgentsList() {
  try {
    agents.value = await listAgents()
  } catch (e) {
    console.error(e)
  }
}

async function pickWorkspaceFolder() {
  if (!isTauriRuntime()) return
  const conv = chat.current || chat.newConversation()
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const dir = await open({ directory: true, multiple: false })
    if (typeof dir === 'string' && dir) {
      chat.setConversationWorkspace(dir)
    }
  } catch (e) {
    console.error(e)
  }
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

function remindWorkspaceRequired() {
  showWorkspaceRequiredModal.value = true
}

async function onWorkspaceRequiredPick() {
  showWorkspaceRequiredModal.value = false
  if (isTauriRuntime()) {
    await pickWorkspaceFolder()
    return
  }
  nextTick(() => workspaceInputRef.value?.focus())
}

function send() {
  if (!canSend.value) return
  if (needsWorkspace.value && !hasWorkspace.value) {
    remindWorkspaceRequired()
    return
  }
  void sendWithOptionalComputerScreenPick()
}

const showScreenPicker = ref(false)
const showWorkspaceRequiredModal = ref(false)
const screenPickerLoading = ref(false)
const screenPickerError = ref<string | null>(null)
const screenPickerMonitors = ref<ComputerMonitor[]>([])
const pendingSendText = ref<string | null>(null)

async function sendWithOptionalComputerScreenPick() {
  const conv = chat.current || chat.newConversation()
  const v = text.value
  if (!v.trim()) return

  if (showComputerMonitorPicker.value) {
    try {
      screenPickerError.value = null
      screenPickerLoading.value = true
      const monitors = await listComputerMonitors()
      screenPickerMonitors.value = monitors
      screenPickerLoading.value = false

      if (!conv.computerMonitorId) {
        if (monitors.length > 1) {
          pendingSendText.value = v
          showScreenPicker.value = true
          return
        }
        if (monitors.length === 1) {
          conv.computerMonitorId = monitors[0].id
        }
      }

      await setComputerConversationMonitor(conv.id, conv.computerMonitorId || null)
    } catch (e: any) {
      screenPickerLoading.value = false
      screenPickerError.value = String(e?.message || e)
      pendingSendText.value = v
      showScreenPicker.value = true
      return
    }
  }

  text.value = ''
  chat.sendUserMessage(v)
  nextTick(() => {
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
  })
}

async function onPickScreen(monitorId: string) {
  const conv = chat.current || chat.newConversation()
  conv.computerMonitorId = monitorId
  try {
    await setComputerConversationMonitor(conv.id, monitorId)
  } catch (e) {
    // If setting fails, keep the picker open with error so the user can retry.
    screenPickerError.value = String((e as any)?.message || e)
    return
  }
  showScreenPicker.value = false
  const v = pendingSendText.value
  pendingSendText.value = null
  if (!v) return
  text.value = ''
  chat.sendUserMessage(v)
  nextTick(() => {
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
  })
}

function onKeydown(e: KeyboardEvent) {
  if (e.isComposing || composing.value) return
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    send()
  }
}

function onCompositionEnd() {
  setTimeout(() => {
    composing.value = false
  }, 50)
}

async function selectWorkerAgent(agent: AgentDef) {
  await settings.saveAgentPreferences({ agentMode: 'single', leadAgentId: agent.id })
  showAgentPicker.value = false
}

function autoResize() {
  if (!textareaRef.value) return
  textareaRef.value.style.height = 'auto'
  textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 250) + 'px'
}

function updatePickerWidths() {
  if (agentBtnRef.value) {
    const btnWidth = agentBtnRef.value.offsetWidth
    agentPickerWidth.value = Math.max(btnWidth, 200)
  }
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (showAgentPicker.value && agentBtnRef.value && agentPickerRef.value) {
    if (!agentBtnRef.value.contains(target) && !agentPickerRef.value.contains(target)) {
      showAgentPicker.value = false
    }
  }
}

watch(
  () => showAgentPicker.value,
  () => nextTick(() => updatePickerWidths())
)

onMounted(() => {
  loadAgentsList()
  if (!TEAM_MODE_UI_ENABLED && settings.settings.agentMode === 'supervisor') {
    void settings.saveAgentPreferences({
      agentMode: 'single',
      leadAgentId: settings.settings.leadAgentId || DEFAULT_LEAD_AGENT_ID
    })
  }
  updatePickerWidths()
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <ComputerScreenPickerModal
    v-model:open="showScreenPicker"
    :monitors="screenPickerMonitors"
    :loading="screenPickerLoading"
    :error="screenPickerError"
    @pick="onPickScreen"
  />

  <WorkspaceRequiredModal
    v-model:open="showWorkspaceRequiredModal"
    :is-desktop="isTauriRuntime()"
    @pick="onWorkspaceRequiredPick"
  />

  <div class="px-6 md:px-10 pb-5">
    <div class="max-w-3xl mx-auto">
      <div v-if="showLoginBanner" class="mb-2 flex w-fit max-w-full flex-col gap-1.5">
        <div
          class="inline-flex max-w-full flex-wrap items-center gap-3 rounded-xl border border-accent/20 bg-accent-muted/40 px-3.5 py-2.5"
        >
          <p class="shrink-0 text-xs leading-snug text-foreground">未登录，登录后可继续对话</p>
          <PlatformLoginActions
            variant="compact"
            :loading="platformAuth.loading"
            :error="null"
            @login="onPlatformLogin"
            @cancel="onPlatformLoginCancel"
          />
        </div>
        <p
          v-if="platformAuth.error"
          class="max-w-full rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
          role="alert"
        >
          {{ platformAuth.error }}
        </p>
      </div>

      <div class="panel-elevated rounded-2xl border border-border overflow-visible px-2 pb-2 pt-[18px]">
        <textarea
          ref="textareaRef"
          v-model="text"
          rows="1"
          class="block w-full resize-none bg-transparent border-0 outline-none px-3 pt-[3px] pb-2 text-[15px] text-foreground placeholder:text-muted"
          style="max-height: 250px; min-height: 24px;"
          :placeholder="composerPlaceholder"
          :disabled="needsPlatformLogin || tokenQuotaBlocked"
          @keydown="onKeydown"
          @input="autoResize"
          @compositionstart="composing = true"
          @compositionend="onCompositionEnd"
        />

        <div class="flex items-center gap-2">
          <div class="relative flex flex-1 flex-wrap items-center gap-x-3 gap-y-0 min-w-0 px-1">
            <div class="relative">
              <button
                ref="agentBtnRef"
                type="button"
                class="composer-agent-trigger cursor-pointer"
                @click="showAgentPicker = !showAgentPicker"
              >
                <component :is="currentAgentIcon" class="w-3 h-3 shrink-0 text-accent" />
                <span class="truncate">{{ currentAgentLabel }}</span>
                <ChevronDown class="w-3 h-3 shrink-0 text-muted" />
              </button>

              <div
                v-if="showAgentPicker"
                ref="agentPickerRef"
                class="composer-dropdown composer-dropdown--up"
                :style="{ minWidth: agentPickerWidth + 'px' }"
              >
                <div class="px-3 py-2 border-b border-border">
                  <div class="text-[11px] text-muted font-medium">执行智能体</div>
                </div>
                <div class="p-1.5 space-y-0.5 max-h-60 overflow-y-auto">
                  <button
                    v-for="w in workers"
                    :key="w.id"
                    type="button"
                    class="composer-dropdown-item cursor-pointer"
                    :class="isLeadAgentSelected(w.id) ? 'composer-dropdown-item-active' : ''"
                    @click="selectWorkerAgent(w)"
                  >
                    <component :is="iconForAgent(w, settings.settings)" class="w-3 h-3 shrink-0" />
                    <span class="min-w-0 truncate">{{ composerAgentLabel(w, settings.settings) }}</span>
                    <span class="text-[10px] text-muted shrink-0">{{ w.id }}</span>
                  </button>
                </div>
              </div>
            </div>

            <div v-if="needsWorkspace" class="relative flex items-center gap-1 min-w-0">
              <button
                v-if="isTauriRuntime()"
                type="button"
                class="composer-agent-trigger max-w-[200px] cursor-pointer"
                :title="workspaceTooltip"
                @click="pickWorkspaceFolder"
              >
                <FolderOpen class="w-3 h-3 shrink-0 text-warning" />
                <span class="truncate max-w-[150px]">{{ workspaceDirName || '工作目录…' }}</span>
              </button>
              <input
                v-else
                ref="workspaceInputRef"
                :value="chat.current?.workspaceRoot ?? ''"
                type="text"
                placeholder="工作目录"
                class="composer-workspace-input"
                :title="workspaceTooltip"
                @input="onWorkspaceInput"
              />
              <button
                v-if="hasWorkspace"
                type="button"
                class="composer-agent-trigger px-1 py-1 text-muted hover:text-foreground cursor-pointer"
                title="清除工作目录"
                @click="clearWorkspace"
              >
                <X class="w-3 h-3 shrink-0" />
              </button>
            </div>
          </div>

          <button
            v-if="chat.generating"
            class="h-10 w-10 shrink-0 rounded-xl bg-danger/20 hover:bg-danger/30 text-danger flex items-center justify-center cursor-pointer transition"
            @click="chat.stop()"
            title="停止"
          ><Square class="w-4 h-4" /></button>
          <button
            v-else
            class="h-10 w-10 shrink-0 rounded-xl flex items-center justify-center transition"
            :class="canSend
              ? 'bg-accent text-white hover:opacity-90 cursor-pointer'
              : 'bg-hover text-muted cursor-not-allowed'"
            :disabled="!canSend"
            @click="send"
          ><Send class="w-4 h-4" /></button>
        </div>
      </div>



      <div
        v-if="supervisorRoundsLabel || !settings.settings.hasKey"
        class="flex flex-wrap items-center gap-2 mt-2"
      >
        <div class="flex-1" />

        <span v-if="supervisorRoundsLabel" class="text-[10px] text-muted">{{ supervisorRoundsLabel }}</span>
        <span v-if="!settings.settings.hasKey" class="text-[10px] text-muted">未配置 Key</span>
      </div>
    </div>
  </div>
</template>
