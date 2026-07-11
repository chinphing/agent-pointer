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
import { useAgentsCatalog } from '../../composables/useAgentUi'
import type { AgentDef, ComputerMonitor, ComputerMonitorPickRequest, ComposerAttachment } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID } from '../../types/chat'
import {
  getMacosComputerPermissions,
  listComputerMonitors,
  readLocalFileForAttachment,
  setComputerConversationMonitor,
  confirmComputerMonitorPick,
  cancelComputerMonitorPick
} from '../../lib/api'
import { getLocalFileSize } from '../../lib/tauri'
import { detectDesktopOs } from '../../lib/desktopOs'
import {
  clearMacosComputerPermissionsUserAck,
  hasMacosComputerPermissionsUserAck
} from '../../lib/macosPermissionsSession'
import { isTauriRuntime } from '../../lib/runtime'
import {
  CHAT_ATTACHMENT_ACCEPT,
  composerVideoCompressConfirmMessage,
  composerVideoCompressHint,
  dataUrlToBase64,
  isLargeComposerVideo,
  isSupportedChatAttachmentFile,
  isVideoAttachmentFile,
  mediaKindFromFile
} from '../../lib/attachmentSupport'
import {
  cloneComposerAttachmentsForSend,
  registerComposerAttachmentPayload,
  releaseComposerAttachment
} from '../../lib/attachmentPayloadStore'
import { isMediaOssConfigured, uploadComposerVideoToOss, formatVideoOssInvokeError, getMediaOssUploadStatus } from '../../lib/videoOssUpload'
import OutboundQueuePanel from './OutboundQueuePanel.vue'
import { videoPreviewUrlFromLocalPath, videoPreviewUrlFromStorage } from '../../lib/chatMediaPreview'
import type { MacosComputerPermissionsStatus } from '../../types/macosPermissions'
import ComputerScreenPickerModal from './ComputerScreenPickerModal.vue'
import { primaryComputerMonitor } from '../../lib/computerMonitorLayout'
import AttachmentChip from './AttachmentChip.vue'
import MacosComputerPermissionsModal from './MacosComputerPermissionsModal.vue'

function isEphemeralWorkspacePath(path: string): boolean {
  const normalized = path.replace(/\\/g, '/')
  return normalized.includes('/session-sandboxes/') || normalized.includes('/coder-sandboxes/')
}

const COMPOSER_TEXTAREA_MAX_HEIGHT_PX = 250
const COMPOSER_TEXTAREA_MIN_HEIGHT_PX = 24

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
const { composerPrefill, composerText, composerAttachments, generating, currentOutboundQueue } = storeToRefs(chat)

const tokenQuotaBlocked = computed(() => platformAuth.tokenQuotaExhausted)
const needsPlatformLogin = computed(() => !platformAuth.session.logged_in)
const showLoginBanner = computed(
  () => needsPlatformLogin.value && (chat.current?.messages.length ?? 0) > 0
)
const composerPlaceholder = computed(() => {
  if (needsPlatformLogin.value) {
    return platformAuth.isStandalone ? '请先登录' : '请先登录 Pointer 账户'
  }
  if (tokenQuotaBlocked.value) {
    return '账户余额已用尽，请前往官网余额页充值'
  }
  return settings.settings.hasKey ? '告诉我你想做什么' : '请先在设置中配置 API Key'
})

const composing = ref(false)
const showAgentPicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const agentBtnRef = ref<HTMLButtonElement | null>(null)
const agentPickerRef = ref<HTMLDivElement | null>(null)
const workspaceInputRef = ref<HTMLInputElement | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)
const composerDropZoneRef = ref<HTMLDivElement | null>(null)
const attachmentHint = ref<string | null>(null)
const composerDragDepth = ref(0)
const isComposerDragOver = computed(() => composerDragDepth.value > 0)
/** Desktop: `onUnmounted` must call this — each Composer instance registers its own listener. */
let unlistenTauriDragDrop: (() => void) | null = null

const agents = useAgentsCatalog()

const sessionAgentMode = computed(() => chat.effectiveConversationAgentMode(chat.current))
const sessionLeadAgentId = computed(() => chat.effectiveConversationLeadAgentId(chat.current))

const sessionAgentSettings = computed(() => ({
  ...settings.settings,
  agentMode: sessionAgentMode.value,
  leadAgentId: sessionLeadAgentId.value
}))

const leadUi = computed(() => resolveLeadAgentUi(sessionAgentSettings.value, agents.value))

const workers = computed(() => {
  const list = agents.value.filter(a => {
    if (a.role !== 'worker' || !a.enabled) return false
    return resolveAgentUi(a, settings.settings).userSelectable
  })
  return sortComposerAgents(list)
})

const selectedWorker = computed(() => {
  if (sessionAgentMode.value !== 'single') return undefined
  const id = sessionLeadAgentId.value
  return workers.value.find(w => w.id === id) ?? workers.value.find(w => w.id === DEFAULT_LEAD_AGENT_ID)
})

const selectedWorkerId = computed(() => selectedWorker.value?.id?.trim() || DEFAULT_LEAD_AGENT_ID)

function isLeadAgentSelected(agentId: string): boolean {
  if (sessionAgentMode.value !== 'single') return false
  return sessionLeadAgentId.value === agentId
}

const showComputerMonitorPicker = computed(
  () => sessionAgentMode.value === 'single' && leadUi.value.showComputerMonitorPicker
)

const computerAutoSwitchMonitor = computed(
  () => settings.settings.computerAutoSwitchMonitor !== false
)

const isMacDesktop = computed(
  () => isTauriRuntime() && detectDesktopOs() === 'macos'
)

const showWorkspacePicker = computed(() => true)

const supervisorRoundsLabel = computed(() => {
  if (sessionAgentMode.value !== 'supervisor' || !chat.current) return ''
  const used = chat.current.toolRoundsUsedSupervisor ?? 0
  const max = settings.settings.maxSubAgentToolRounds ?? settings.settings.maxToolRounds ?? 100
  return `子任务轮次 ${used}/${max}`
})

const workspaceDirName = computed(() => {
  const p = chat.current?.workspaceRoot ?? ''
  if (!p) return ''
  if (isEphemeralWorkspacePath(p)) return '临时目录'
  return p.replace(/[/\\]+$/, '').split(/[/\\]/).pop() || ''
})

const workspaceTooltip = computed(() => {
  const p = chat.current?.workspaceRoot?.trim()
  if (!p) return '留空时将继承上一会话工作目录；清除后发送则使用临时目录'
  if (isEphemeralWorkspacePath(p)) return `临时工作目录：${p}`
  return p
})

const currentAgentLabel = computed(() => composerAgentLabel(selectedWorker.value, sessionAgentSettings.value))

const currentAgentIcon = computed(() => iconForAgent(selectedWorker.value, sessionAgentSettings.value))

const hasWorkspace = computed(() => !!(chat.current?.workspaceRoot?.trim()))

const workspaceNeedsAttention = computed(() => {
  const p = chat.current?.workspaceRoot?.trim() ?? ''
  return !p || isEphemeralWorkspacePath(p)
})

const canSend = computed(() => {
  const attachments = composerAttachments.value
  const videoBlocked = attachments.some(
    a =>
      a.kind === 'video' &&
      (a.uploadState === 'compressing' ||
        a.uploadState === 'uploading' ||
        a.uploadState === 'pending' ||
        a.uploadState === 'error' ||
        !a.remoteUrl?.trim())
  )
  return (
    ((composerText.value.length > 0 && composerText.value.trim().length > 0) ||
      attachments.length > 0) &&
    !needsPlatformLogin.value &&
    !tokenQuotaBlocked.value &&
    settings.settings.hasKey &&
    !videoBlocked
  )
})

const outboundQueueList = currentOutboundQueue

async function onPlatformLogin() {
  try {
    await platformAuth.login()
    chat.clearPlatformLoginErrorMessages()
  } catch {
    /* error in store */
  }
}

function onLocalLoginSuccess() {
  chat.clearPlatformLoginErrorMessages()
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
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

function send() {
  if (!canSend.value) return
  void sendWithOptionalComputerScreenPick()
}

const showScreenPicker = ref(false)
const showPermissionsModal = ref(false)
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

function pushComposerAttachment(
  file: File,
  dataUrl: string,
  contentBase64: string,
  meta: Pick<ComposerAttachment, 'kind' | 'mimeType' | 'fileName' | 'sizeBytes'>
) {
  const attachment: ComposerAttachment = {
    id: uid(),
    kind: meta.kind,
    mimeType: meta.mimeType,
    fileName: meta.fileName,
    sizeBytes: meta.sizeBytes
  }
  composerAttachments.value.push(
    registerComposerAttachmentPayload({ attachment, dataUrl, contentBase64, file })
  )
}

function updateComposerAttachment(id: string, patch: Partial<ComposerAttachment>) {
  composerAttachments.value = composerAttachments.value.map(a =>
    a.id === id ? { ...a, ...patch } : a
  )
}

async function resolveComposerVideoPreviewUrl(
  storageRelPath?: string,
  localSourcePath?: string
): Promise<string | undefined> {
  if (storageRelPath?.trim()) {
    const fromStorage = await videoPreviewUrlFromStorage(storageRelPath)
    if (fromStorage) return fromStorage
  }
  if (localSourcePath?.trim()) {
    const fromLocal = await videoPreviewUrlFromLocalPath(localSourcePath)
    if (fromLocal) return fromLocal
  }
  return undefined
}

async function startVideoOssUpload(
  attachment: ComposerAttachment,
  file: File,
  localPath: string | undefined,
  compress: boolean
) {
  updateComposerAttachment(attachment.id, {
    uploadState: compress ? 'compressing' : 'uploading',
    uploadProgress: 0,
    uploadError: undefined
  })
  try {
    const result = await uploadComposerVideoToOss(
      attachment.id,
      file,
      localPath,
      progress => {
        updateComposerAttachment(attachment.id, {
          uploadProgress: progress.percent,
          uploadState: 'uploading'
        })
      },
      { compress, conversationId: chat.current?.id }
    )
    const previewUrl = await resolveComposerVideoPreviewUrl(
      result.storageRelPath,
      localPath
    )
    updateComposerAttachment(attachment.id, {
      remoteUrl: result.remoteUrl,
      ossObjectKey: result.ossObjectKey,
      storageRelPath: result.storageRelPath,
      ...(previewUrl ? { previewUrl } : {}),
      uploadState: 'done',
      uploadProgress: 100,
      uploadError: undefined
    })
  } catch (err) {
    const message = formatVideoOssInvokeError(err)
    console.error('video OSS upload failed', err)
    updateComposerAttachment(attachment.id, {
      uploadState: 'error',
      uploadError: message
    })
    attachmentHint.value = message
  }
}

async function ensureVideoOssReady(): Promise<string | null> {
  if (isTauriRuntime()) {
    await settings.load()
    const status = await getMediaOssUploadStatus()
    if (!status.configured) {
      return status.message ?? '视频上传需要平台 OSS 配置，请登录 Pointer 账户或联系管理员在官网配置 OSS'
    }
    return null
  }
  if (!isMediaOssConfigured(settings.settings)) {
    return '视频上传需要平台 OSS 配置，请登录 Pointer 账户或联系管理员在官网配置 OSS'
  }
  return null
}

async function addVideoAttachment(file: File, localPath?: string) {
  const ossBlock = await ensureVideoOssReady()
  if (ossBlock) {
    attachmentHint.value = ossBlock
    return
  }
  const fileName = file.name?.trim() || localPath?.split(/[/\\]/).pop() || 'video.mp4'
  let sizeBytes = file.size
  if (sizeBytes <= 0 && localPath?.trim() && isTauriRuntime()) {
    try {
      sizeBytes = await getLocalFileSize(localPath)
    } catch (err) {
      console.warn('video attachment: stat local file failed', err)
    }
  }

  let compress = false
  if (isLargeComposerVideo(sizeBytes)) {
    const confirmed = window.confirm(composerVideoCompressConfirmMessage(fileName, sizeBytes))
    if (!confirmed) {
      attachmentHint.value = null
      return
    }
    compress = true
    attachmentHint.value = composerVideoCompressHint(fileName)
  } else {
    attachmentHint.value = null
  }

  const attachment: ComposerAttachment = {
    id: uid(),
    kind: 'video',
    mimeType: file.type || 'video/mp4',
    fileName,
    sizeBytes,
    uploadState: 'pending',
    uploadProgress: 0,
    ...(localPath?.trim() ? { localSourcePath: localPath.trim() } : {})
  }
  const previewUrl =
    (await resolveComposerVideoPreviewUrl(undefined, localPath)) ??
    (typeof URL !== 'undefined' && typeof URL.createObjectURL === 'function' && file.size > 0
      ? URL.createObjectURL(file)
      : undefined)
  composerAttachments.value.push({
    ...attachment,
    ...(previewUrl ? { previewUrl } : {})
  })
  await startVideoOssUpload(
    attachment,
    file.size > 0 ? file : new File([], fileName),
    localPath,
    compress
  )
}

async function addAttachmentFile(file: File) {
  attachmentHint.value = null
  if (!isSupportedChatAttachmentFile(file)) {
    attachmentHint.value = `无法添加附件：${file.name}`
    console.warn('unsupported attachment', file.name, file.type)
    return
  }
  if (isVideoAttachmentFile(file)) {
    if (isTauriRuntime()) {
      attachmentHint.value = '请使用附件按钮（回形针）选择视频文件'
      return
    }
    await addVideoAttachment(file)
    return
  }
  const dataUrl = await readFileAsDataUrl(file)
  const contentBase64 = dataUrlToBase64(dataUrl)
  pushComposerAttachment(file, dataUrl, contentBase64, {
    kind: mediaKindFromFile(file),
    mimeType: file.type || 'application/octet-stream',
    fileName: file.name,
    sizeBytes: file.size
  })
}

async function addAttachmentFromLocalPath(path: string) {
  attachmentHint.value = null
  const name = path.split(/[/\\]/).pop() || 'attachment'
  const fileLike = { name, type: '', size: 0 }
  if (!isSupportedChatAttachmentFile(fileLike)) {
    attachmentHint.value = `无法添加附件：${name}`
    return
  }
  if (isVideoAttachmentFile(fileLike)) {
    const placeholder = new File([], name)
    await addVideoAttachment(placeholder, path)
    return
  }
  const payload = await readLocalFileForAttachment(path)
  const loaded = {
    name: payload.fileName,
    type: payload.mimeType,
    size: payload.sizeBytes
  }
  if (!isSupportedChatAttachmentFile(loaded)) {
    attachmentHint.value = `无法添加附件：${payload.fileName}`
    return
  }
  const mime = payload.mimeType || 'application/octet-stream'
  const bytes = Uint8Array.from(atob(payload.contentBase64), c => c.charCodeAt(0))
  const blob = new Blob([bytes], { type: mime })
  const file = new File([blob], payload.fileName, { type: mime })
  const dataUrl = `data:${mime};base64,${payload.contentBase64}`
  pushComposerAttachment(file, dataUrl, payload.contentBase64, {
    kind: mediaKindFromFile(loaded),
    mimeType: mime,
    fileName: payload.fileName,
    sizeBytes: payload.sizeBytes
  })
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

function canAcceptComposerAttachments(): boolean {
  return !needsPlatformLogin.value && !tokenQuotaBlocked.value && settings.settings.hasKey
}

function composerAttachmentBlockedHint(): string {
  if (needsPlatformLogin.value) {
    return platformAuth.isStandalone ? '请先登录后再添加附件' : '请先登录 Pointer 账户后再添加附件'
  }
  if (tokenQuotaBlocked.value) return '账户余额已用尽，暂无法添加附件'
  if (!settings.settings.hasKey) return '请先在设置中配置 API Key'
  return '当前无法添加附件'
}

/*
 * Composer file drag-and-drop (Web + Tauri)
 *
 * Two mutually exclusive paths — do not merge into one handler:
 *
 * - Web: HTML5 `@drop` on `composerDropZoneRef` (below in template). Uses `File` API.
 * - Desktop (Tauri): `webview.onDragDropEvent` in `setupTauriComposerDragDrop`.
 *   Tauri intercepts OS file drags; HTML5 `drop` does NOT fire for Finder/Explorer files.
 *   Payload gives filesystem paths (`ingestDroppedPaths`), not bytes.
 *
 * Common regressions (see docs/contributing/web-media-and-desktop-snapshot.md):
 * - Setting `dragDropEnabled: false` in tauri.conf.json — breaks native drops on macOS;
 *   HTML5 fallback is unreliable in Tauri WebView. Keep default `true`.
 * - DOMRect / `getBoundingClientRect` hit tests on `event.payload.position` — coords are
 *   window-outer relative; frameless + macOS overlay title bar ≠ viewport (tauri#10744).
 *   Accept window-level drops instead of coordinate targeting.
 * - Removing `if (isTauriRuntime()) return` from HTML5 handlers — dead on desktop but documents
 *   intent; do not wire desktop file intake only through `@drop`.
 * - Calling `webview.scaleFactor()` — not on Webview type; use `getCurrentWindow().scaleFactor()`
 *   if physical→logical conversion is ever needed again.
 * - Changing `canAcceptComposerAttachments` without UI hint — blocked drops look like "no response".
 */

function isLikelyFileDrag(dt: DataTransfer | null | undefined): boolean {
  if (!dt) return false
  if (dt.files?.length) return true
  const types = Array.from(dt.types || [])
  if (types.includes('Files')) return true
  if (types.some(t => /file|url|uri-list/i.test(t))) return true
  return Array.from(dt.items || []).some(item => item.kind === 'file')
}

async function ingestDroppedPaths(paths: string[]) {
  // Desktop native drop only — paths from Tauri, not from HTML5 `dataTransfer`.
  if (!paths.length) return
  if (!canAcceptComposerAttachments()) {
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
  attachmentHint.value = null
  for (const path of paths) {
    try {
      await addAttachmentFromLocalPath(path)
    } catch (err) {
      console.error('drop attachment from path failed', path, err)
      attachmentHint.value = formatVideoOssInvokeError(err) || `无法读取文件：${path}`
    }
  }
  nextTick(() => textareaRef.value?.focus())
}

async function ingestDroppedFiles(files: File[]) {
  if (!files.length) return
  if (!canAcceptComposerAttachments()) {
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
  attachmentHint.value = null
  for (const file of files) {
    try {
      await addDroppedAttachmentFile(file)
    } catch (err) {
      console.error('drop attachment failed', err)
    }
  }
  nextTick(() => textareaRef.value?.focus())
}

function droppedFileLocalPath(file: File): string | undefined {
  const path = (file as File & { path?: string }).path?.trim()
  return path || undefined
}

async function addDroppedAttachmentFile(file: File) {
  if (isTauriRuntime()) {
    const localPath = droppedFileLocalPath(file)
    if (localPath) {
      try {
        await addAttachmentFromLocalPath(localPath)
        return
      } catch (err) {
        console.warn('dropped file path attach failed, falling back to file read', localPath, err)
      }
    }
  }
  await addAttachmentFile(file)
}

function onComposerDragEnter(e: DragEvent) {
  // Web only — see block comment above; Tauri uses `onDragDropEvent`.
  if (isTauriRuntime()) return
  if (!isLikelyFileDrag(e.dataTransfer)) return
  e.preventDefault()
  e.stopPropagation()
  if (canAcceptComposerAttachments()) {
    composerDragDepth.value += 1
  }
}

function onComposerDragOver(e: DragEvent) {
  if (isTauriRuntime()) return
  if (!isLikelyFileDrag(e.dataTransfer)) return
  e.preventDefault()
  e.stopPropagation()
  if (!canAcceptComposerAttachments()) {
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'none'
    return
  }
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
}

function onComposerDragLeave(e: DragEvent) {
  if (isTauriRuntime()) return
  const el = e.currentTarget as HTMLElement
  const related = e.relatedTarget as Node | null
  if (related && el.contains(related)) return
  composerDragDepth.value = Math.max(0, composerDragDepth.value - 1)
}

async function onComposerDrop(e: DragEvent) {
  if (isTauriRuntime()) return
  composerDragDepth.value = 0
  e.preventDefault()
  e.stopPropagation()
  const files = e.dataTransfer?.files
  if (!files?.length) return
  if (!canAcceptComposerAttachments()) {
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
  await ingestDroppedFiles(Array.from(files))
}

async function setupTauriComposerDragDrop() {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    const webview = getCurrentWebview()
    // Window-level listener: do NOT filter by pointer position (unreliable on overlay chrome).
    // Accept any drop with paths while this Composer is mounted.
    unlistenTauriDragDrop = await webview.onDragDropEvent(async event => {
      const payload = event.payload
      if (payload.type === 'enter' || payload.type === 'over') {
        composerDragDepth.value = canAcceptComposerAttachments() ? 1 : 0
        return
      }
      if (payload.type === 'drop') {
        composerDragDepth.value = 0
        if (!payload.paths?.length) return
        if (!canAcceptComposerAttachments()) {
          attachmentHint.value = composerAttachmentBlockedHint()
          console.warn('[composer-drag-drop] drop blocked:', composerAttachmentBlockedHint())
          return
        }
        console.info('[composer-drag-drop] native drop', payload.paths)
        await ingestDroppedPaths(payload.paths)
        return
      }
      composerDragDepth.value = 0
    })
    console.info('[composer-drag-drop] native listener ready')
  } catch (err) {
    console.warn('tauri composer drag-drop listener failed', err)
  }
}

function removePendingAttachment(id: string) {
  composerAttachments.value = composerAttachments.value.filter(a => a.id !== id)
  releaseComposerAttachment(id)
}

async function openAttachmentPicker() {
  if (isTauriRuntime()) {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ multiple: true })
      if (!selected) return
      const paths = typeof selected === 'string' ? [selected] : selected
      for (const path of paths) {
        try {
          await addAttachmentFromLocalPath(path)
        } catch (err) {
          console.error('attachment from path failed', path, err)
          attachmentHint.value = formatVideoOssInvokeError(err) || `无法读取文件：${path}`
        }
      }
      return
    } catch (e) {
      console.warn('tauri file dialog failed, falling back to input', e)
    }
  }
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
  const attachments = cloneComposerAttachmentsForSend(composerAttachments.value)
  chat.clearActiveComposer()
  chat.sendUserMessage(textValue, attachments)
}

async function sendWithOptionalComputerScreenPick() {
  const conv = chat.current || chat.newConversation()
  const v = composerText.value
  if (!v.trim() && composerAttachments.value.length === 0) return

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

      if (computerAutoSwitchMonitor.value) {
        const primary = primaryComputerMonitor(monitors)
        if (primary) conv.computerMonitorId = primary.id
        await setComputerConversationMonitor(conv.id, conv.computerMonitorId || null)
      } else if (!conv.computerMonitorId) {
        if (monitors.length > 1) {
          pendingSendText.value = v
          showScreenPicker.value = true
          return
        }
        if (monitors.length === 1) {
          conv.computerMonitorId = monitors[0].id
        }
        await setComputerConversationMonitor(conv.id, conv.computerMonitorId || null)
      } else {
        await setComputerConversationMonitor(conv.id, conv.computerMonitorId)
      }
    } catch (e: any) {
      screenPickerLoading.value = false
      screenPickerError.value = String(e?.message || e)
      pendingSendText.value = v
      showScreenPicker.value = true
      return
    }
  }

  dispatchSend(v)
  nextTick(() => autoResize())
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
  composerText.value = v
  send()
}

async function confirmSubagentMonitorPick(
  req: ComputerMonitorPickRequest,
  conv: { id: string; computerMonitorId?: string },
  monitorId: string
) {
  conv.computerMonitorId = monitorId
  await setComputerConversationMonitor(conv.id, monitorId)
  await confirmComputerMonitorPick(req.conversationId)
  chat.clearComputerMonitorPickRequest()
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

  try {
    if (computerAutoSwitchMonitor.value) {
      const primary = primaryComputerMonitor(req.monitors)
      if (primary) {
        await confirmSubagentMonitorPick(req, conv, primary.id)
        return
      }
    } else if (conv.computerMonitorId) {
      await confirmSubagentMonitorPick(req, conv, conv.computerMonitorId)
      return
    } else if (req.monitors.length === 1) {
      await confirmSubagentMonitorPick(req, conv, req.monitors[0].id)
      return
    } else if (req.monitors.length > 1) {
      screenPickerError.value = null
      screenPickerLoading.value = false
      screenPickerMonitors.value = req.monitors
      subagentPickPending.value = req
      showScreenPicker.value = true
      return
    }
    throw new Error('未检测到可用屏幕')
  } catch (e: unknown) {
    screenPickerError.value = String((e as { message?: string })?.message || e)
    screenPickerMonitors.value = req.monitors
    subagentPickPending.value = req
    showScreenPicker.value = true
  }
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
  dispatchSend(v)
  nextTick(() => autoResize())
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

function selectWorkerAgent(agent: AgentDef) {
  chat.setConversationAgent(agent.id, 'single')
  showAgentPicker.value = false
}

let composerResizeRaf: number | null = null

function autoResize() {
  if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
  composerResizeRaf = requestAnimationFrame(() => {
    composerResizeRaf = null
    const el = textareaRef.value
    if (!el) return

    const max = COMPOSER_TEXTAREA_MAX_HEIGHT_PX
    const atMax =
      el.offsetHeight >= max - 1 && el.scrollHeight > max

    if (atMax) {
      el.style.height = `${max}px`
      el.style.overflowY = 'auto'
      return
    }

    el.style.overflowY = 'hidden'
    el.style.height = 'auto'
    const nextHeight = Math.min(
      Math.max(el.scrollHeight, COMPOSER_TEXTAREA_MIN_HEIGHT_PX),
      max
    )
    el.style.height = `${nextHeight}px`
    el.style.overflowY = el.scrollHeight > max ? 'auto' : 'hidden'
  })
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
  composerText.value = draft
  chat.consumeComposerPrefill()
  nextTick(() => {
    autoResize()
    textareaRef.value?.focus()
  })
})

onMounted(() => {
  nextTick(autoResize)
  if (!TEAM_MODE_UI_ENABLED && sessionAgentMode.value === 'supervisor') {
    chat.setConversationAgent(DEFAULT_LEAD_AGENT_ID, 'single')
  }
  if (
    !RESEARCH_COMPOSER_UI_ENABLED
    && chat.current
    && chat.effectiveConversationLeadAgentId(chat.current) === 'research'
  ) {
    chat.setConversationAgent(DEFAULT_LEAD_AGENT_ID, 'single')
  }
  document.addEventListener('click', handleClickOutside)
  void setupTauriComposerDragDrop() // no-op on Web; required on desktop — see drag-and-drop comment block
})

onUnmounted(() => {
  if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
  document.removeEventListener('click', handleClickOutside)
  unlistenTauriDragDrop?.()
  unlistenTauriDragDrop = null
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
            @local-success="onLocalLoginSuccess"
          />
        </div>
      </div>

      <OutboundQueuePanel
        v-if="chat.current?.id"
        :conversation-id="chat.current.id"
        :items="outboundQueueList"
      />

      <!-- Web: HTML5 file drop on this zone. Desktop: drag highlight only; file intake is setupTauriComposerDragDrop. -->
      <div
        ref="composerDropZoneRef"
        class="panel-elevated rounded-2xl border overflow-visible px-2 pb-2 pt-[18px] transition-colors"
        :class="isComposerDragOver ? 'border-accent/50 bg-accent-muted/15' : 'border-border'"
        @dragenter.capture="onComposerDragEnter"
        @dragover.capture="onComposerDragOver"
        @dragleave.capture="onComposerDragLeave"
        @drop.capture="onComposerDrop"
      >
        <input
          ref="fileInputRef"
          type="file"
          class="hidden"
          multiple
          :accept="CHAT_ATTACHMENT_ACCEPT"
          @change="onAttachmentFiles"
        />
        <div
          v-if="composerAttachments.length"
          class="flex flex-wrap gap-2 px-3 pb-2"
        >
          <AttachmentChip
            v-for="att in composerAttachments"
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
          v-model="composerText"
          rows="1"
          class="block w-full resize-none bg-transparent border-0 outline-none px-3 pt-[3px] pb-2 text-[15px] text-foreground placeholder:text-muted"
          :style="{
            maxHeight: `${COMPOSER_TEXTAREA_MAX_HEIGHT_PX}px`,
            minHeight: `${COMPOSER_TEXTAREA_MIN_HEIGHT_PX}px`
          }"
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

            <div v-if="showWorkspacePicker" class="relative flex items-center gap-1 min-w-0">
              <button
                v-if="isTauriRuntime()"
                type="button"
                class="composer-agent-trigger max-w-[200px] cursor-pointer"
                :title="workspaceTooltip"
                @click="pickWorkspaceFolder"
              >
                <FolderOpen
                  class="w-3 h-3 shrink-0"
                  :class="workspaceNeedsAttention ? 'text-warning' : 'text-muted'"
                />
                <span class="truncate max-w-[150px]">{{ workspaceDirName || '工作目录…' }}</span>
              </button>
              <input
                v-else
                ref="workspaceInputRef"
                :value="chat.current?.workspaceRoot ?? ''"
                type="text"
                placeholder="项目目录（留空继承上一会话；清除后发送用临时目录）"
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
            v-if="generating"
            class="h-10 w-10 shrink-0 rounded-xl bg-danger/20 hover:bg-danger/30 text-danger flex items-center justify-center cursor-pointer transition"
            @click="chat.stop()"
            title="停止当前任务"
          ><Square class="w-4 h-4" /></button>
          <button
            class="h-10 w-10 shrink-0 rounded-xl flex items-center justify-center transition"
            :class="canSend
              ? 'bg-accent text-white hover:opacity-90 cursor-pointer'
              : 'bg-hover text-muted cursor-not-allowed'"
            :disabled="!canSend"
            :title="generating ? '加入发送队列' : '发送'"
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
