<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { ChevronDown, FolderOpen, Paperclip, Send, Square, X } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { resolveAgentUi, resolveLeadAgentUi, composerAgentLabel, RESEARCH_COMPOSER_UI_ENABLED } from '../../lib/agentUi'
import { iconForAgent, sortComposerAgents, TEAM_MODE_UI_ENABLED } from '../../lib/agentIcons'
import type { AgentDef, ComputerMonitor, ComputerMonitorPickRequest, ComposerAttachment } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../../types/chat'
import {
  getMacosComputerPermissions,
  listAgents,
  listComputerMonitors,
  setComputerConversationMonitor,
  confirmComputerMonitorPick,
  cancelComputerMonitorPick
} from '../../lib/api'
import { detectDesktopOs } from '../../lib/desktopOs'
import {
  clearMacosComputerPermissionsUserAck,
  hasMacosComputerPermissionsUserAck
} from '../../lib/macosPermissionsSession'
import { isTauriRuntime } from '../../lib/runtime'
import {
  CHAT_ATTACHMENT_ACCEPT,
  composerVideoSizeError,
  dataUrlToBase64,
  isSupportedChatAttachmentFile,
  mediaKindFromFile
} from '../../lib/attachmentSupport'
import {
  cloneComposerAttachmentsForSend,
  registerComposerAttachmentPayload,
  releaseComposerAttachment
} from '../../lib/attachmentPayloadStore'
import type { MacosComputerPermissionsStatus } from '../../types/macosPermissions'
import ComputerScreenPickerModal from './ComputerScreenPickerModal.vue'
import AttachmentChip from './AttachmentChip.vue'
import MacosComputerPermissionsModal from './MacosComputerPermissionsModal.vue'
import WorkspaceRequiredModal from './WorkspaceRequiredModal.vue'

const props = withDefaults(
  defineProps<{
    /** footer: fixed bottom bar; inline: embedded in welcome hero */
    placement?: 'footer' | 'inline'
  }>(),
  { placement: 'footer' }
)

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const { composerPrefill } = storeToRefs(chat)

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
  return settings.settings.hasKey ? '告诉我你想做什么' : '请先在设置中配置 API Key'
})

const text = ref('')
const composing = ref(false)
const showAgentPicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const agentBtnRef = ref<HTMLButtonElement | null>(null)
const agentPickerRef = ref<HTMLDivElement | null>(null)
const workspaceInputRef = ref<HTMLInputElement | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)
const pendingAttachments = ref<ComposerAttachment[]>([])
const attachmentHint = ref<string | null>(null)

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

const isMacDesktop = computed(
  () => isTauriRuntime() && detectDesktopOs() === 'macos'
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
    (text.value.trim().length > 0 || pendingAttachments.value.length > 0) &&
    !chat.generating &&
    !needsPlatformLogin.value &&
    !tokenQuotaBlocked.value &&
    settings.settings.hasKey
)

async function onPlatformLogin() {
  try {
    await platformAuth.login()
    chat.clearPlatformLoginErrorMessages()
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
const showPermissionsModal = ref(false)
const showWorkspaceRequiredModal = ref(false)
const screenPickerLoading = ref(false)
const screenPickerError = ref<string | null>(null)
const screenPickerMonitors = ref<ComputerMonitor[]>([])
const pendingSendText = ref<string | null>(null)
const subagentPickPending = ref<ComputerMonitorPickRequest | null>(null)
const subagentPickResolved = ref(false)
const pendingSubagentMonitorPick = ref<ComputerMonitorPickRequest | null>(null)

function macosComputerPermissionsAllowSend(perms: MacosComputerPermissionsStatus): boolean {
  if (perms.screenRecording && perms.accessibility) {
    clearMacosComputerPermissionsUserAck()
    return true
  }
  return hasMacosComputerPermissionsUserAck()
}

function uid() {
  return crypto.randomUUID?.() ?? `att-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

async function readFileAsDataUrl(file: File): Promise<string> {
  return await new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.readAsDataURL(file)
  })
}

async function addAttachmentFile(file: File) {
  attachmentHint.value = null
  if (!isSupportedChatAttachmentFile(file)) {
    attachmentHint.value = `不支持该文件类型：${file.name}`
    console.warn('unsupported attachment', file.name, file.type)
    return
  }
  const videoSizeError = composerVideoSizeError(file)
  if (videoSizeError) {
    attachmentHint.value = videoSizeError
    return
  }
  const dataUrl = await readFileAsDataUrl(file)
  const contentBase64 = dataUrlToBase64(dataUrl)
  const attachment: ComposerAttachment = {
    id: uid(),
    kind: mediaKindFromFile(file),
    mimeType: file.type || 'application/octet-stream',
    fileName: file.name,
    sizeBytes: file.size
  }
  pendingAttachments.value.push(
    registerComposerAttachmentPayload({ attachment, dataUrl, contentBase64, file })
  )
}

async function onAttachmentFiles(e: Event) {
  const input = e.target as HTMLInputElement
  const files = input.files ? Array.from(input.files) : []
  input.value = ''
  for (const file of files) {
    try {
      await addAttachmentFile(file)
    } catch (err) {
      console.error('attachment add failed', err)
    }
  }
}

function removePendingAttachment(id: string) {
  pendingAttachments.value = pendingAttachments.value.filter(a => a.id !== id)
  releaseComposerAttachment(id)
}

function openAttachmentPicker() {
  fileInputRef.value?.click()
}

async function onPasteAttachments(e: ClipboardEvent) {
  const items = e.clipboardData?.items
  if (!items?.length) return
  for (const item of items) {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) continue
    const file = item.getAsFile()
    if (!file) continue
    e.preventDefault()
    try {
      await addAttachmentFile(file)
    } catch (err) {
      console.error('paste attachment failed', err)
    }
  }
}

function dispatchSend(textValue: string) {
  const attachments = cloneComposerAttachmentsForSend(pendingAttachments.value)
  pendingAttachments.value = []
  chat.sendUserMessage(textValue, attachments)
}

async function sendWithOptionalComputerScreenPick() {
  const conv = chat.current || chat.newConversation()
  const v = text.value
  if (!v.trim() && pendingAttachments.value.length === 0) return

  if (showComputerMonitorPicker.value && isMacDesktop.value) {
    try {
      const perms = await getMacosComputerPermissions()
      if (!macosComputerPermissionsAllowSend(perms)) {
        pendingSendText.value = v
        showPermissionsModal.value = true
        return
      }
    } catch (e: unknown) {
      pendingSendText.value = v
      showPermissionsModal.value = true
      return
    }
  }

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
  dispatchSend(v)
  nextTick(() => {
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
  })
}

async function onPermissionsReady() {
  const subReq = pendingSubagentMonitorPick.value
  if (subReq) {
    pendingSubagentMonitorPick.value = null
    void beginSubagentMonitorPickFlow(subReq)
    return
  }
  const v = pendingSendText.value
  if (!v) return
  pendingSendText.value = null
  text.value = v
  send()
}

async function beginSubagentMonitorPickFlow(req: ComputerMonitorPickRequest) {
  const conv = chat.conversations.find(c => c.id === req.conversationId) ?? chat.current
  if (!conv) {
    chat.clearComputerMonitorPickRequest()
    return
  }

  if (isMacDesktop.value) {
    try {
      const perms = await getMacosComputerPermissions()
      if (!macosComputerPermissionsAllowSend(perms)) {
        pendingSubagentMonitorPick.value = req
        showPermissionsModal.value = true
        return
      }
    } catch {
      pendingSubagentMonitorPick.value = req
      showPermissionsModal.value = true
      return
    }
  }

  if (conv.computerMonitorId) {
    try {
      await setComputerConversationMonitor(conv.id, conv.computerMonitorId)
      await confirmComputerMonitorPick(req.conversationId)
      chat.clearComputerMonitorPickRequest()
    } catch (e: unknown) {
      screenPickerError.value = String((e as { message?: string })?.message || e)
      screenPickerMonitors.value = req.monitors
      subagentPickPending.value = req
      showScreenPicker.value = true
    }
    return
  }

  screenPickerError.value = null
  screenPickerLoading.value = false
  screenPickerMonitors.value = req.monitors
  subagentPickPending.value = req
  showScreenPicker.value = true
}

watch(
  () => chat.computerMonitorPickRequest,
  req => {
    if (req) void beginSubagentMonitorPickFlow(req)
  }
)

watch(showScreenPicker, (open, wasOpen) => {
  if (open || !wasOpen) return
  const req = subagentPickPending.value
  if (!req) return
  if (subagentPickResolved.value) {
    subagentPickResolved.value = false
    subagentPickPending.value = null
    return
  }
  subagentPickPending.value = null
  chat.clearComputerMonitorPickRequest()
  void cancelComputerMonitorPick(req.conversationId)
})

async function onPickScreen(monitorId: string) {
  const subReq = subagentPickPending.value
  const conv = subReq
    ? (chat.conversations.find(c => c.id === subReq.conversationId) ?? chat.current)
    : (chat.current || chat.newConversation())
  if (!conv) return
  conv.computerMonitorId = monitorId
  try {
    await setComputerConversationMonitor(conv.id, monitorId)
  } catch (e) {
    screenPickerError.value = String((e as { message?: string })?.message || e)
    return
  }
  if (subReq) {
    try {
      await confirmComputerMonitorPick(subReq.conversationId)
    } catch (e) {
      screenPickerError.value = String((e as { message?: string })?.message || e)
      return
    }
    subagentPickResolved.value = true
    showScreenPicker.value = false
    chat.clearComputerMonitorPickRequest()
    return
  }
  showScreenPicker.value = false
  const v = pendingSendText.value
  pendingSendText.value = null
  if (!v) return
  text.value = ''
  dispatchSend(v)
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

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (showAgentPicker.value && agentBtnRef.value && agentPickerRef.value) {
    if (!agentBtnRef.value.contains(target) && !agentPickerRef.value.contains(target)) {
      showAgentPicker.value = false
    }
  }
}

watch(composerPrefill, (draft) => {
  if (!draft?.trim()) return
  text.value = draft
  chat.consumeComposerPrefill()
  nextTick(() => {
    autoResize()
    textareaRef.value?.focus()
  })
})

onMounted(() => {
  loadAgentsList()
  if (!TEAM_MODE_UI_ENABLED && settings.settings.agentMode === 'supervisor') {
    void settings.saveAgentPreferences({
      agentMode: 'single',
      leadAgentId: settings.settings.leadAgentId || DEFAULT_LEAD_AGENT_ID
    })
  }
  if (!RESEARCH_COMPOSER_UI_ENABLED && settings.settings.leadAgentId === 'research') {
    void settings.saveAgentPreferences({
      agentMode: 'single',
      leadAgentId: DEFAULT_LEAD_AGENT_ID
    })
  }
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <MacosComputerPermissionsModal
    v-if="isMacDesktop"
    v-model:open="showPermissionsModal"
    @ready="onPermissionsReady"
  />

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

  <div
    :class="props.placement === 'inline'
      ? 'w-full'
      : 'chat-shell shrink-0 bg-background pt-2 pb-5'"
  >
    <div :class="props.placement === 'inline' ? 'w-full' : 'chat-column'">
      <div v-if="showLoginBanner" class="mb-2 flex w-fit max-w-full flex-col gap-1.5">
        <div
          class="inline-flex max-w-full flex-wrap items-center gap-3 rounded-xl border border-accent/20 bg-accent-muted/40 px-3.5 py-2.5"
        >
          <p class="shrink-0 text-xs leading-snug text-foreground">
            {{ platformAuth.error || '未登录，登录后可继续对话' }}
          </p>
          <PlatformLoginActions
            variant="compact"
            :loading="platformAuth.loading"
            :error="null"
            @login="onPlatformLogin"
            @cancel="onPlatformLoginCancel"
          />
        </div>
      </div>

      <div class="panel-elevated rounded-2xl border border-border overflow-visible px-2 pb-2 pt-[18px]">
        <input
          ref="fileInputRef"
          type="file"
          class="hidden"
          multiple
          :accept="CHAT_ATTACHMENT_ACCEPT"
          @change="onAttachmentFiles"
        />
        <div
          v-if="pendingAttachments.length"
          class="flex flex-wrap gap-2 px-3 pb-2"
        >
          <AttachmentChip
            v-for="att in pendingAttachments"
            :key="att.id"
            :attachment="att"
            @remove="removePendingAttachment(att.id)"
          />
        </div>
        <p
          v-if="attachmentHint"
          class="px-3 pb-2 text-[11px] text-amber-600"
        >
          {{ attachmentHint }}
        </p>
        <textarea
          ref="textareaRef"
          v-model="text"
          rows="1"
          class="block w-full resize-none bg-transparent border-0 outline-none px-3 pt-[3px] pb-2 text-[15px] text-foreground placeholder:text-muted"
          :style="{ maxHeight: '250px', minHeight: '24px' }"
          :placeholder="composerPlaceholder"
          :disabled="needsPlatformLogin || tokenQuotaBlocked"
          @keydown="onKeydown"
          @input="autoResize"
          @paste="onPasteAttachments"
          @compositionstart="composing = true"
          @compositionend="onCompositionEnd"
        />

        <div class="flex items-center gap-2">
          <div class="relative flex flex-1 flex-wrap items-center gap-x-3 gap-y-0 min-w-0 px-1">
            <button
              type="button"
              class="composer-agent-trigger cursor-pointer"
              title="添加附件"
              @click="openAttachmentPicker"
            >
              <Paperclip class="w-3 h-3 shrink-0 text-muted" />
            </button>
            <div class="relative">
              <button
                ref="agentBtnRef"
                type="button"
                class="composer-agent-trigger cursor-pointer"
                @click="showAgentPicker = !showAgentPicker"
              >
                <component :is="currentAgentIcon" class="w-3 h-3 shrink-0 text-accent" />
                <span class="whitespace-nowrap">{{ currentAgentLabel }}</span>
                <ChevronDown class="w-3 h-3 shrink-0 text-muted" />
              </button>

              <div
                v-if="showAgentPicker"
                ref="agentPickerRef"
                class="composer-dropdown composer-dropdown--fit"
                :class="props.placement === 'inline' ? 'composer-dropdown--down' : 'composer-dropdown--up'"
              >
                <div class="px-2 py-1.5 border-b border-border">
                  <div class="text-[11px] text-muted font-medium whitespace-nowrap">执行智能体</div>
                </div>
                <div class="p-1 space-y-0.5 max-h-60 overflow-y-auto">
                  <button
                    v-for="w in workers"
                    :key="w.id"
                    type="button"
                    class="composer-dropdown-item composer-dropdown-item--compact cursor-pointer"
                    :class="isLeadAgentSelected(w.id) ? 'composer-dropdown-item-active' : ''"
                    @click="selectWorkerAgent(w)"
                  >
                    <component :is="iconForAgent(w, settings.settings)" class="w-3 h-3 shrink-0" />
                    <span class="whitespace-nowrap">{{ composerAgentLabel(w, settings.settings) }}</span>
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
