<script setup lang="ts">
import { ref, computed, inject, onMounted, onUnmounted, nextTick, watch } from 'vue'
import type { Component } from 'vue'
import { storeToRefs } from 'pinia'
import { Check, ChevronDown, Gauge, Paperclip, Rocket, Send, Settings2, Square, Zap } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useSettingsStore } from '../../stores/settings'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { resolveAgentUi, resolveLeadAgentUi, composerAgentLabel } from '../../lib/agentUi'
import { iconForAgent, sortComposerAgents } from '../../lib/agentIcons'
import { useAgentsCatalog } from '../../composables/useAgentUi'
import type { AgentDef, ComputerMonitor, ComputerMonitorPickRequest, ComposerAttachment, PerformanceMode } from '../../types/chat'
import { DEFAULT_LEAD_AGENT_ID, PERFORMANCE_MODE_OPTIONS } from '../../types/chat'
import {
  getMacosComputerPermissions,
  listComputerMonitors,
  readLocalFileForAttachment,
  saveChatAttachment,
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
  acquireComposerTauriDragDrop,
  normalizeComposerDropPath
} from '../../lib/composerTauriDragDrop'
import { openPlatformBillingPage } from '../../lib/platformUrls'
import {
  CHAT_ATTACHMENT_ACCEPT,
  composerVideoCompressConfirmMessage,
  composerVideoCompressHint,
  composerAttachmentTooLargeMessage,
  composerAttachmentUploadMaxBytes,
  dataUrlToBase64,
  isLargeComposerVideo,
  isSupportedChatAttachmentFile,
  isVideoAttachmentFile,
  mediaKindFromFile
} from '../../lib/attachmentSupport'
import {
  cloneComposerAttachmentsForSend,
  getComposerAttachmentContentBase64,
  getComposerAttachmentFile,
  registerComposerAttachmentFile,
  registerComposerAttachmentPayload,
  releaseComposerAttachment
} from '../../lib/attachmentPayloadStore'
import { maybeCompressImageFile } from '../../lib/imageCompress'
import { isUploadAbortedError, UPLOAD_ABORTED_MESSAGE } from '../../lib/multipartUpload'
import { withRetries } from '../../lib/retry'
import { isMediaOssConfigured, uploadComposerVideoToOss, formatVideoOssInvokeError, getMediaOssUploadStatus } from '../../lib/videoOssUpload'
import OutboundQueuePanel from './OutboundQueuePanel.vue'
import { videoPreviewUrlFromLocalPath, videoPreviewUrlFromStorage } from '../../lib/chatMediaPreview'
import type { MacosComputerPermissionsStatus } from '../../types/macosPermissions'
import ComputerScreenPickerModal from './ComputerScreenPickerModal.vue'
import { primaryComputerMonitor } from '../../lib/computerMonitorLayout'
import AttachmentChip from './AttachmentChip.vue'
import MacosComputerPermissionsModal from './MacosComputerPermissionsModal.vue'
import { OpenSettingsKey } from '../../lib/settingsDialogKey'
import { resolveComposerPlaceholder } from '../../lib/webBranding'
import { randomUuid } from '../../lib/randomUuid'

const COMPOSER_TEXTAREA_MAX_HEIGHT_PX = 250
/** ≈ one line with py-2 + leading-5; must not stay on :style during measure. */
const COMPOSER_TEXTAREA_MIN_HEIGHT_PX = 36

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
    return platformAuth.loginHint()
  }
  if (tokenQuotaBlocked.value) {
    return '账户余额已用尽'
  }
  return settings.settings.hasKey ? resolveComposerPlaceholder() : '请先在设置中配置 API Key'
})

const composing = ref(false)
const showAgentPicker = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)
const agentBtnRef = ref<HTMLButtonElement | null>(null)
const agentPickerRef = ref<HTMLDivElement | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)
const composerDropZoneRef = ref<HTMLDivElement | null>(null)
const attachmentHint = ref<string | null>(null)

function attachmentUploadLimitBytes(): number {
  return composerAttachmentUploadMaxBytes(settings.settings.attachmentUploadMaxBytes)
}

function rejectOversizedNonVideo(fileName: string, sizeBytes: number): boolean {
  if (isVideoAttachmentFile({ name: fileName, type: '' })) return false
  const limit = attachmentUploadLimitBytes()
  if (sizeBytes > limit) {
    attachmentHint.value = composerAttachmentTooLargeMessage(fileName, limit)
    return true
  }
  return false
}
const composerDragDepth = ref(0)
const isComposerDragOver = computed(() => composerDragDepth.value > 0)
/** Disposer for the shared Tauri window drop listener (see composerTauriDragDrop). */
let releaseTauriDragDrop: (() => void) | null = null
/** Guard async acquire against unmount mid-setup (avoids leaking a handler on the stack). */
let composerDragDropMounted = false

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

/** Shortcut labels: prefer OS (incl. web), not only Tauri desktop. */
const isMacOs = computed(() => detectDesktopOs() === 'macos')

const currentAgentLabel = computed(() => composerAgentLabel(selectedWorker.value, sessionAgentSettings.value))

const currentAgentIcon = computed(() => iconForAgent(selectedWorker.value, sessionAgentSettings.value))

const canSend = computed(() => {
  const attachments = composerAttachments.value
  const uploadBlocked = attachments.some(a => {
    if (
      a.uploadState === 'compressing' ||
      a.uploadState === 'uploading' ||
      a.uploadState === 'pending' ||
      a.uploadState === 'error'
    ) {
      return true
    }
    if (a.kind === 'video') return !a.remoteUrl?.trim()
    return !a.storageRelPath?.trim()
  })
  return (
    ((composerText.value.length > 0 && composerText.value.trim().length > 0) ||
      attachments.length > 0) &&
    !needsPlatformLogin.value &&
    !tokenQuotaBlocked.value &&
    settings.settings.hasKey &&
    !uploadBlocked
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

async function onOpenBilling() {
  try {
    await openPlatformBillingPage()
  } catch (e) {
    console.warn('[composer] open billing page failed', e)
  }
}


// ── 模式设置（快速/标准/高级）──
const openSettings = inject(OpenSettingsKey, undefined)
const modePickerOpen = ref(false)
const modePickerButtonRef = ref<HTMLButtonElement | null>(null)
const modePickerRef = ref<HTMLDivElement | null>(null)

const performanceMode = computed(() => {
  const convMode = chat.current?.performanceMode
  if (convMode === 'fast' || convMode === 'standard' || convMode === 'expert') return convMode
  return settings.getAgentPerformanceMode(sessionLeadAgentId.value)
})

/** 模式档位图标：快速 ⚡ / 标准 仪表 / 高级 火箭 */
const PERFORMANCE_MODE_ICONS: Record<PerformanceMode, Component> = {
  fast: Zap,
  standard: Gauge,
  expert: Rocket
}

const performanceModeIcon = computed(() => PERFORMANCE_MODE_ICONS[performanceMode.value])

const performanceModeLabel = computed(
  () => PERFORMANCE_MODE_OPTIONS.find(o => o.value === performanceMode.value)?.label ?? '标准'
)

function selectMode(mode: PerformanceMode) {
  chat.setConversationPerformanceMode(mode)
  modePickerOpen.value = false
}

function openAgentSettings() {
  modePickerOpen.value = false
  openSettings?.('assistant')
}

function send() {
  if (!canSend.value) return
  void sendWithOptionalComputerScreenPick()
}

function hasComposerDraft(): boolean {
  return (
    (composerText.value.length > 0 && composerText.value.trim().length > 0) ||
    composerAttachments.value.length > 0
  )
}

/**
 * Cursor-style stop & send: interrupt the active turn and dispatch immediately.
 * - With draft text/attachments: enqueue then force-send that item.
 * - Empty draft + non-empty queue: force-send the first queued item.
 */
async function stopAndSendNow() {
  const conv = chat.current || chat.newConversation()
  const convId = conv.id
  const wasGenerating = !!generating.value || chat.isConversationGenerating(convId)
  const queueBefore = chat.outboundQueueItems(convId)

  if (hasComposerDraft()) {
    if (!canSend.value) return
    // Bypass screen-pick flow: stop-and-send must interrupt immediately like Cursor.
    const text = composerText.value
    const attachments = cloneComposerAttachmentsForSend(composerAttachments.value)
    chat.clearActiveComposer()
    await chat.sendUserMessage(text, attachments)
    if (!wasGenerating) return
    const queue = chat.outboundQueueItems(convId)
    const newlyQueued =
      queue.find(item => !queueBefore.some(prev => prev.id === item.id)) ??
      queue[queue.length - 1]
    if (newlyQueued) {
      await chat.forceSendOutbound(convId, newlyQueued.id)
    }
    return
  }

  if (queueBefore.length > 0) {
    await chat.forceSendOutbound(convId, queueBefore[0]!.id)
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.isComposing || composing.value) return
  if (e.key !== 'Enter' || e.shiftKey) return

  // Cursor: Cmd/Ctrl+Enter → stop current turn and send immediately.
  if (e.metaKey || e.ctrlKey) {
    e.preventDefault()
    void stopAndSendNow()
    return
  }

  e.preventDefault()

  // Cursor: plain Enter with empty input while queue has items → send queue head now.
  if (!hasComposerDraft() && outboundQueueList.value.length > 0) {
    void stopAndSendNow()
    return
  }

  send()
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
  return randomUuid()
}

async function readFileAsDataUrl(file: File): Promise<string> {
  return await new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.readAsDataURL(file)
  })
}

function updateComposerAttachment(id: string, patch: Partial<ComposerAttachment>) {
  composerAttachments.value = composerAttachments.value.map(a =>
    a.id === id ? { ...a, ...patch } : a
  )
}

/** In-flight upload abort + video compress choice for retry. */
const attachmentUploadControllers = new Map<string, AbortController>()
const videoCompressByAttachmentId = new Map<string, boolean>()

function beginAttachmentUpload(attachmentId: string): AbortSignal {
  attachmentUploadControllers.get(attachmentId)?.abort()
  const controller = new AbortController()
  attachmentUploadControllers.set(attachmentId, controller)
  return controller.signal
}

function endAttachmentUpload(attachmentId: string, signal?: AbortSignal) {
  const current = attachmentUploadControllers.get(attachmentId)
  if (!current) return
  if (signal && current.signal !== signal) return
  attachmentUploadControllers.delete(attachmentId)
}

function isActiveAttachmentUpload(attachmentId: string, signal: AbortSignal): boolean {
  const current = attachmentUploadControllers.get(attachmentId)
  // No controller: cancelled via abortAttachmentUpload (still apply terminal error).
  if (!current) return true
  return current.signal === signal
}

function abortAttachmentUpload(attachmentId: string) {
  const controller = attachmentUploadControllers.get(attachmentId)
  if (!controller) return
  controller.abort()
  attachmentUploadControllers.delete(attachmentId)
}

function ensureComposerConversationId(): string {
  if (!chat.current) chat.newConversation()
  const id = chat.current?.id?.trim()
  if (!id) throw new Error('无法创建会话，附件上传中止')
  return id
}

function formatAttachmentPersistError(err: unknown): string {
  if (isUploadAbortedError(err)) return UPLOAD_ABORTED_MESSAGE
  const mapped = platformAuth.formatLoginGateError(err, 'attachment')
  return mapped || '上传失败'
}

/** Persist non-video attachment (multipart on web; invoke+base64 on desktop). */
async function persistComposerAttachment(
  attachmentId: string,
  source: { file?: File; contentBase64?: string }
) {
  const row = composerAttachments.value.find(a => a.id === attachmentId)
  if (!row || row.kind === 'video') return
  const signal = beginAttachmentUpload(attachmentId)
  updateComposerAttachment(attachmentId, {
    uploadState: 'pending',
    uploadProgress: 0,
    uploadError: undefined
  })
  try {
    await platformAuth.requireSession({ purpose: 'attachment', onTransient: 'allow' })
    if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
    const conversationId = ensureComposerConversationId()
    let uploadFile = source.file
    let contentBase64 = source.contentBase64
    if (row.kind === 'image' && uploadFile && !isTauriRuntime()) {
      updateComposerAttachment(attachmentId, { uploadState: 'compressing', uploadProgress: 0 })
      uploadFile = await maybeCompressImageFile(uploadFile)
      if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
      if (uploadFile.type === 'image/jpeg' && uploadFile.name.endsWith('.jpg')) {
        updateComposerAttachment(attachmentId, {
          mimeType: uploadFile.type,
          fileName: uploadFile.name,
          sizeBytes: uploadFile.size
        })
      }
    }
    updateComposerAttachment(attachmentId, { uploadState: 'uploading', uploadProgress: 0 })
    if (!isTauriRuntime()) {
      if (!uploadFile) throw new Error('缺少上传文件')
      const fileForUpload = uploadFile
      const storageRelPath = await withRetries(
        async () =>
          await saveChatAttachment(
            {
              conversationId,
              attachmentId,
              fileName: fileForUpload.name || row.fileName,
              file: fileForUpload
            },
            p => {
              if (signal.aborted) return
              updateComposerAttachment(attachmentId, {
                uploadProgress: p.percent,
                uploadState: 'uploading',
                uploadError: undefined
              })
            },
            { signal }
          ),
        {
          onRetry: (_err, nextAttempt, _delayMs) => {
            if (signal.aborted) return
            updateComposerAttachment(attachmentId, {
              uploadState: 'uploading',
              uploadProgress: 0,
              uploadError: `重试中 ${nextAttempt}/3…`
            })
          }
        }
      )
      if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
      if (!isActiveAttachmentUpload(attachmentId, signal)) return
      updateComposerAttachment(attachmentId, {
        storageRelPath,
        uploadState: 'done',
        uploadProgress: 100,
        uploadError: undefined
      })
      return
    }
    if (!contentBase64?.trim()) {
      if (!uploadFile) throw new Error('缺少附件内容')
      const dataUrl = await readFileAsDataUrl(uploadFile)
      contentBase64 = dataUrlToBase64(dataUrl)
    }
    if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
    const b64 = contentBase64
    const storageRelPath = await withRetries(
      async () =>
        await saveChatAttachment(
          {
            conversationId,
            attachmentId,
            fileName: row.fileName,
            contentBase64: b64
          },
          p => {
            if (signal.aborted) return
            updateComposerAttachment(attachmentId, {
              uploadProgress: p.percent,
              uploadState: 'uploading',
              uploadError: undefined
            })
          },
          { signal }
        ),
      {
        onRetry: (_err, nextAttempt) => {
          if (signal.aborted) return
          updateComposerAttachment(attachmentId, {
            uploadState: 'uploading',
            uploadProgress: 0,
            uploadError: `重试中 ${nextAttempt}/3…`
          })
        }
      }
    )
    if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
    if (!isActiveAttachmentUpload(attachmentId, signal)) return
    updateComposerAttachment(attachmentId, {
      storageRelPath,
      uploadState: 'done',
      uploadProgress: 100,
      uploadError: undefined
    })
  } catch (err) {
    if (isUploadAbortedError(err)) {
      console.info('[composer] attachment upload cancelled', attachmentId)
    } else {
      console.error('[composer] attachment persist failed', err)
    }
    if (!composerAttachments.value.some(a => a.id === attachmentId)) return
    if (!isActiveAttachmentUpload(attachmentId, signal)) return
    updateComposerAttachment(attachmentId, {
      uploadState: 'error',
      uploadError: formatAttachmentPersistError(err)
    })
  } finally {
    endAttachmentUpload(attachmentId, signal)
  }
}

async function addNonVideoFileOptimistic(file: File) {
  const kind = mediaKindFromFile(file)
  const attachment: ComposerAttachment = {
    id: uid(),
    kind,
    mimeType: file.type || 'application/octet-stream',
    fileName: file.name,
    sizeBytes: file.size,
    uploadState: 'pending',
    uploadProgress: 0
  }
  composerAttachments.value.push(registerComposerAttachmentFile({ attachment, file }))
  void persistComposerAttachment(attachment.id, { file })
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
  if (rejectOversizedNonVideo(file.name, file.size)) return
  // Instant chip via object URL; persist in background (multipart on web).
  await addNonVideoFileOptimistic(file)
}

async function addAttachmentFromLocalPath(path: string) {
  attachmentHint.value = null
  const name = path.split(/[/\\]/).pop() || 'attachment'
  const normalizedPath = normalizeComposerDropPath(path)
  if (
    composerAttachments.value.some(
      a => a.localSourcePath && normalizeComposerDropPath(a.localSourcePath) === normalizedPath
    )
  ) {
    console.info('[composer] skip duplicate local attachment path', path)
    return
  }
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
  try {
    const sizeBytes = await getLocalFileSize(path)
    if (rejectOversizedNonVideo(name, sizeBytes)) return
  } catch (err) {
    console.warn('[composer] getLocalFileSize failed', path, err)
  }
  const attachmentId = uid()
  const pending: ComposerAttachment = {
    id: attachmentId,
    kind: mediaKindFromFile(fileLike),
    mimeType: 'application/octet-stream',
    fileName: name,
    sizeBytes: 0,
    uploadState: 'pending',
    uploadProgress: 0,
    localSourcePath: path
  }
  composerAttachments.value.push(pending)
  try {
    const payload = await readLocalFileForAttachment(path)
    const loaded = {
      name: payload.fileName,
      type: payload.mimeType,
      size: payload.sizeBytes
    }
    const kind = mediaKindFromFile(loaded)
    const mime = payload.mimeType || 'application/octet-stream'
    const dataUrl = `data:${mime};base64,${payload.contentBase64}`
    registerComposerAttachmentPayload({
      attachment: {
        id: attachmentId,
        kind,
        mimeType: mime,
        fileName: payload.fileName || name,
        sizeBytes: payload.sizeBytes
      },
      dataUrl,
      contentBase64: payload.contentBase64
    })
    updateComposerAttachment(attachmentId, {
      kind,
      mimeType: mime,
      fileName: payload.fileName || name,
      sizeBytes: payload.sizeBytes,
      previewUrl: dataUrl
    })
    await persistComposerAttachment(attachmentId, { contentBase64: payload.contentBase64 })
  } catch (err) {
    console.error('attachment from path failed', path, err)
    updateComposerAttachment(attachmentId, {
      uploadState: 'error',
      uploadError: formatAttachmentPersistError(err)
    })
  }
}

async function onAttachmentFiles(e: Event) {
  const input = e.target as HTMLInputElement
  const files = input.files ? Array.from(input.files) : []
  input.value = ''
  await Promise.all(
    files.map(async file => {
      try {
        await addAttachmentFile(file)
      } catch (err) {
        console.error('attachment add failed', err)
      }
    })
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
  videoCompressByAttachmentId.set(attachment.id, compress)
  const signal = beginAttachmentUpload(attachment.id)
  updateComposerAttachment(attachment.id, {
    uploadState: compress ? 'compressing' : 'uploading',
    uploadProgress: 0,
    uploadError: undefined
  })
  try {
    const result = await withRetries(
      async () =>
        await uploadComposerVideoToOss(
          attachment.id,
          file,
          localPath,
          progress => {
            if (signal.aborted) return
            updateComposerAttachment(attachment.id, {
              uploadProgress: progress.percent,
              uploadState: 'uploading',
              uploadError: undefined
            })
          },
          { compress, conversationId: chat.current?.id, signal }
        ),
      {
        onRetry: (_err, nextAttempt) => {
          if (signal.aborted) return
          updateComposerAttachment(attachment.id, {
            uploadState: 'uploading',
            uploadProgress: 0,
            uploadError: `重试中 ${nextAttempt}/3…`
          })
        }
      }
    )
    if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
    if (!isActiveAttachmentUpload(attachment.id, signal)) return
    const previewUrl = await resolveComposerVideoPreviewUrl(
      result.storageRelPath,
      localPath
    )
    if (signal.aborted) throw new Error(UPLOAD_ABORTED_MESSAGE)
    if (!isActiveAttachmentUpload(attachment.id, signal)) return
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
    if (isUploadAbortedError(err)) {
      console.info('[composer] video upload cancelled', attachment.id)
    } else {
      console.error('video OSS upload failed', err)
    }
    if (!composerAttachments.value.some(a => a.id === attachment.id)) return
    if (!isActiveAttachmentUpload(attachment.id, signal)) return
    updateComposerAttachment(attachment.id, {
      uploadState: 'error',
      uploadError: isUploadAbortedError(err)
        ? UPLOAD_ABORTED_MESSAGE
        : formatVideoOssInvokeError(err)
    })
  } finally {
    endAttachmentUpload(attachment.id, signal)
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

  const uploadFile = file.size > 0 ? file : new File([], fileName)
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
  const previewFromPath = await resolveComposerVideoPreviewUrl(undefined, localPath)
  const registered = registerComposerAttachmentFile({ attachment, file: uploadFile })
  composerAttachments.value.push({
    ...registered,
    ...(previewFromPath ? { previewUrl: previewFromPath } : {})
  })
  await startVideoOssUpload(attachment, uploadFile, localPath, compress)
}

function cancelComposerAttachmentUpload(attachmentId: string) {
  const row = composerAttachments.value.find(a => a.id === attachmentId)
  if (!row) return
  if (
    row.uploadState !== 'pending' &&
    row.uploadState !== 'compressing' &&
    row.uploadState !== 'uploading'
  ) {
    return
  }
  abortAttachmentUpload(attachmentId)
  updateComposerAttachment(attachmentId, {
    uploadState: 'error',
    uploadError: UPLOAD_ABORTED_MESSAGE
  })
  console.info('[composer] user cancelled attachment upload', attachmentId)
}

async function retryComposerAttachmentUpload(attachmentId: string) {
  const row = composerAttachments.value.find(a => a.id === attachmentId)
  if (!row || row.uploadState !== 'error') return
  if (row.kind === 'video') {
    const file = getComposerAttachmentFile(row) ?? new File([], row.fileName)
    const compress = videoCompressByAttachmentId.get(attachmentId) === true
    console.info('[composer] user retry video upload', attachmentId)
    await startVideoOssUpload(row, file, row.localSourcePath, compress)
    return
  }
  const file = getComposerAttachmentFile(row) ?? undefined
  const contentBase64 = getComposerAttachmentContentBase64(row) ?? undefined
  if (!file && !contentBase64?.trim()) {
    updateComposerAttachment(attachmentId, {
      uploadState: 'error',
      uploadError: '无法重传：缺少本地文件'
    })
    console.warn('[composer] retry missing payload', attachmentId)
    return
  }
  console.info('[composer] user retry attachment upload', attachmentId)
  await persistComposerAttachment(attachmentId, { file, contentBase64 })
}

function canAcceptComposerAttachments(): boolean {
  return !needsPlatformLogin.value && !tokenQuotaBlocked.value && settings.settings.hasKey
}

function composerAttachmentBlockedHint(): string {
  if (needsPlatformLogin.value) {
    return platformAuth.loginHint('attachment')
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
 * - Desktop (Tauri): shared `acquireComposerTauriDragDrop` (`lib/composerTauriDragDrop.ts`).
 *   Tauri intercepts OS file drags; HTML5 `drop` does NOT fire for Finder/Explorer files.
 *   Payload gives filesystem paths (`ingestDroppedPaths`), not bytes.
 *   Listener is module-singleton (not per Composer mount) + short-window path dedupe —
 *   remount races / duplicate native events previously added the same file multiple times
 *   into the shared `composerAttachments` store.
 *
 * Common regressions (see docs/contributing/web-media-and-desktop-snapshot.md):
 * - Setting `dragDropEnabled: false` in tauri.conf.json — breaks native drops on macOS;
 *   HTML5 fallback is unreliable in Tauri WebView. Keep default `true`.
 * - DOMRect / `getBoundingClientRect` hit tests on `event.payload.position` — coords are
 *   window-outer relative; frameless + macOS overlay title bar ≠ viewport (tauri#10744).
 *   Accept window-level drops instead of coordinate targeting.
 * - Registering `onDragDropEvent` inside each Composer instance — leaks / multiplies drops.
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
  await Promise.all(
    paths.map(async path => {
      try {
        await addAttachmentFromLocalPath(path)
      } catch (err) {
        console.error('drop attachment from path failed', path, err)
        attachmentHint.value = formatVideoOssInvokeError(err) || `无法读取文件：${path}`
      }
    })
  )
  nextTick(() => textareaRef.value?.focus())
}

async function ingestDroppedFiles(files: File[]) {
  if (!files.length) return
  if (!canAcceptComposerAttachments()) {
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
  attachmentHint.value = null
  await Promise.all(
    files.map(async file => {
      try {
        await addDroppedAttachmentFile(file)
      } catch (err) {
        console.error('drop attachment failed', err)
      }
    })
  )
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
  // Shared window listener — see lib/composerTauriDragDrop.ts (do not re-register per instance).
  releaseTauriDragDrop?.()
  releaseTauriDragDrop = null
  const release = await acquireComposerTauriDragDrop({
    onHover: active => {
      composerDragDepth.value = active && canAcceptComposerAttachments() ? 1 : 0
    },
    onDrop: async paths => {
      if (!canAcceptComposerAttachments()) {
        attachmentHint.value = composerAttachmentBlockedHint()
        console.warn('[composer-drag-drop] drop blocked:', composerAttachmentBlockedHint())
        return
      }
      await ingestDroppedPaths(paths)
    }
  })
  if (!composerDragDropMounted) {
    release()
    return
  }
  releaseTauriDragDrop = release
}

function removePendingAttachment(id: string) {
  abortAttachmentUpload(id)
  videoCompressByAttachmentId.delete(id)
  const row = composerAttachments.value.find(a => a.id === id)
  if (row?.previewUrl?.startsWith('blob:')) URL.revokeObjectURL(row.previewUrl)
  composerAttachments.value = composerAttachments.value.filter(a => a.id !== id)
  releaseComposerAttachment(id)
}

async function openAttachmentPicker() {
  if (!canAcceptComposerAttachments()) {
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
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
  const files: File[] = []
  for (const item of items) {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) continue
    const file = item.getAsFile()
    if (!file) continue
    files.push(file)
  }
  if (!files.length) return
  if (!canAcceptComposerAttachments()) {
    e.preventDefault()
    attachmentHint.value = composerAttachmentBlockedHint()
    return
  }
  e.preventDefault()
  await Promise.all(
    files.map(async file => {
      try {
        await addAttachmentFile(file)
      } catch (err) {
        console.error('paste attachment failed', err)
      }
    })
  )
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

function onCompositionEnd() {
  setTimeout(() => {
    composing.value = false
    autoResize()
  }, 50)
}

function selectWorkerAgent(agent: AgentDef) {
  chat.setConversationAgent(agent.id, 'single')
  showAgentPicker.value = false
}

let composerResizeRaf: number | null = null
let textareaResizeObserver: ResizeObserver | null = null
/** Last height we wrote on the textarea in the non-field-sizing fallback. */
let composerTextareaHeight = COMPOSER_TEXTAREA_MIN_HEIGHT_PX

/**
 * Grow/shrink the textarea with content up to COMPOSER_TEXTAREA_MAX_HEIGHT_PX.
 *
 * Important: on `md:flex-col`, do not use `flex-1` on the textarea — `flex: 1 1 0%`
 * makes the flex algorithm ignore the JS `height` on the main axis, so the box
 * never appears to grow. Use `md:flex-none` (see template).
 *
 * `force` bypasses the single-line fast path (used when the textarea width
 * changes and soft-wraps may have appeared).
 */
function autoResize(force = false) {
  if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
  composerResizeRaf = requestAnimationFrame(() => {
    composerResizeRaf = null
    const el = textareaRef.value
    if (!el) return

    const max = COMPOSER_TEXTAREA_MAX_HEIGHT_PX
    const min = COMPOSER_TEXTAREA_MIN_HEIGHT_PX

    // Native path (Chromium / recent WebKit): let the engine size to content.
    // CSS already declares field-sizing: content (globals.css), so only apply
    // the inline styles once. Rewriting identical styles on EVERY keystroke
    // marks the textarea style-dirty and forces a layout pass, which can flush
    // pending virtualizer row measurements mid-stream → visible list jump.
    if (typeof CSS !== 'undefined' && CSS.supports?.('field-sizing', 'content')) {
      if (el.dataset.fieldSizingApplied !== '1') {
        el.style.setProperty('field-sizing', 'content')
        el.style.height = 'auto'
        el.style.minHeight = `${min}px`
        el.style.maxHeight = `${max}px`
        el.style.overflowY = 'auto'
        el.dataset.fieldSizingApplied = '1'
      }
      return
    }

    el.style.setProperty('field-sizing', '')
    // Fast path: single-line text that never left the min height does NOT need a
    // layout read. Reading scrollHeight/offsetHeight while the streaming list is
    // mid-measure forces a synchronous reflow in the keystroke frame, flushes the
    // virtualizer's pending row measurements, and fires watch(getTotalSize) →
    // stickScrollerToBottom immediately → the output "jumps" on every keystroke.
    if (!force && !el.value.includes('\n') && composerTextareaHeight === min) {
      return
    }
    el.style.overflowY = 'hidden'
    // Classic height:0 measurement is the only reliable way to read the real
    // content height (scrollHeight is clamped to clientHeight when the box is
    // taller than the content, so direct reads cannot shrink). We only pay the
    // reflow when the height actually needs to change.
    el.style.minHeight = '0'
    el.style.height = '0'
    void el.offsetHeight
    const content = el.scrollHeight
    const nextHeight = Math.min(Math.max(content, min), max)
    el.style.minHeight = `${min}px`
    el.style.height = `${nextHeight}px`
    el.style.overflowY = content > max ? 'auto' : 'hidden'
    composerTextareaHeight = nextHeight
  })
}

function setupTextareaResizeObserver() {
  textareaResizeObserver?.disconnect()
  textareaResizeObserver = null
  const el = textareaRef.value
  if (!el || typeof ResizeObserver === 'undefined') return
  let lastWidth = el.clientWidth
  textareaResizeObserver = new ResizeObserver(() => {
    const node = textareaRef.value
    if (!node) return
    if (node.clientWidth === lastWidth) return
    lastWidth = node.clientWidth
    // Sidebar collapse / window resize changes wrap → re-measure height.
    // force: width change can introduce soft wraps even for single-line text,
    // which the fast path would otherwise skip.
    autoResize(true)
  })
  textareaResizeObserver.observe(el)
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (showAgentPicker.value && agentBtnRef.value && agentPickerRef.value) {
    if (!agentBtnRef.value.contains(target) && !agentPickerRef.value.contains(target)) {
      showAgentPicker.value = false
    }
  }
  if (modePickerOpen.value && modePickerButtonRef.value && modePickerRef.value) {
    if (
      !modePickerButtonRef.value.contains(target) &&
      !modePickerRef.value.contains(target)
    ) {
      modePickerOpen.value = false
    }
  }
}

function handleDocumentKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || !modePickerOpen.value) return
  e.preventDefault()
  modePickerOpen.value = false
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

// Draft restore / clear from the store does not always emit textarea @input.
watch(composerText, () => {
  nextTick(autoResize)
})

onMounted(() => {
  composerDragDropMounted = true
  nextTick(() => {
    autoResize()
    setupTextareaResizeObserver()
  })
  document.addEventListener('click', handleClickOutside)
  document.addEventListener('keydown', handleDocumentKeydown)
  void setupTauriComposerDragDrop() // no-op on Web; required on desktop — see drag-and-drop comment block
})

onUnmounted(() => {
  composerDragDropMounted = false
  if (composerResizeRaf != null) cancelAnimationFrame(composerResizeRaf)
  textareaResizeObserver?.disconnect()
  textareaResizeObserver = null
  document.removeEventListener('click', handleClickOutside)
  document.removeEventListener('keydown', handleDocumentKeydown)
  releaseTauriDragDrop?.()
  releaseTauriDragDrop = null
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
      : 'chat-shell shell-chat shrink-0 pt-2 pb-5'"
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

      <div
        v-else-if="tokenQuotaBlocked && !platformAuth.isStandalone"
        class="mb-2 flex w-fit max-w-full flex-col gap-1.5"
      >
        <div
          class="inline-flex max-w-full flex-wrap items-center gap-3 rounded-xl border border-danger/30 bg-danger/10 px-3.5 py-2.5"
        >
          <p class="shrink-0 text-xs leading-snug text-foreground">
            账户余额已用尽，充值后可继续对话
          </p>
          <button
            type="button"
            class="shrink-0 rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer"
            @click="onOpenBilling"
          >
            去充值
          </button>
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
        class="composer-shell rounded-2xl overflow-visible px-2 transition-colors"
        :class="isComposerDragOver ? 'border-accent/50 bg-accent-muted/15' : ''"
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
          class="flex flex-wrap gap-2 px-3 pb-2 pt-2 md:pt-0"
        >
          <AttachmentChip
            v-for="att in composerAttachments"
            :key="att.id"
            :attachment="att"
            @remove="removePendingAttachment(att.id)"
            @cancel="cancelComposerAttachmentUpload(att.id)"
            @retry="retryComposerAttachmentUpload(att.id)"
          />
        </div>
        <p
          v-if="attachmentHint"
          class="hidden md:block px-3 pb-2 text-[11px] text-warning"
        >
          {{ attachmentHint }}
        </p>
        <div class="composer-body flex items-end gap-1 md:flex-col md:items-stretch md:gap-0">
          <button
            type="button"
            class="composer-agent-trigger mb-0.5 shrink-0 self-end cursor-pointer md:hidden"
            title="添加附件"
            @click="openAttachmentPicker"
          >
            <Paperclip class="w-3.5 h-3.5 shrink-0 text-muted" />
          </button>
          <textarea
            ref="textareaRef"
            v-model="composerText"
            rows="1"
            class="composer-textarea block min-w-0 flex-1 resize-none overflow-x-hidden overflow-y-auto bg-transparent border-0 outline-none px-2 py-2 text-[15px] leading-5 text-foreground placeholder:text-muted md:w-full md:flex-none md:px-3 md:pt-[3px] md:pb-2 md:leading-normal"
            :style="{
              maxHeight: `${COMPOSER_TEXTAREA_MAX_HEIGHT_PX}px`
            }"
            :placeholder="composerPlaceholder"
            :disabled="needsPlatformLogin || tokenQuotaBlocked"
            @keydown="onKeydown"
            @input="autoResize()"
            @paste="onPasteAttachments"
            @compositionstart="composing = true"
            @compositionend="onCompositionEnd"
          />
          <div class="flex shrink-0 items-end gap-2 self-end md:w-full md:items-center md:self-auto">
            <div class="relative hidden min-w-0 flex-1 items-center gap-x-3 px-1 md:flex md:flex-wrap md:gap-y-0">
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
                  <component :is="currentAgentIcon" class="w-3 h-3 shrink-0 text-muted" />
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

              <div class="relative flex min-w-0 items-center">
                <button
                  ref="modePickerButtonRef"
                  type="button"
                  class="composer-agent-trigger"
                  title="模式设置：快速、标准、高级"
                  @click="modePickerOpen = !modePickerOpen"
                >
                  <component :is="performanceModeIcon" class="w-3 h-3 shrink-0 text-muted" />
                  <span class="whitespace-nowrap">{{ performanceModeLabel }}</span>
                  <ChevronDown class="w-3 h-3 shrink-0 text-muted" />
                </button>
                <div
                  v-if="modePickerOpen"
                  ref="modePickerRef"
                  class="composer-dropdown composer-mode-dropdown flex flex-col"
                  :class="props.placement === 'inline' ? 'composer-dropdown--down' : 'composer-dropdown--up'"
                >
                  <!-- 右上角快速入口：进入智能体设置页设置模型 -->
                  <div class="flex items-center justify-between gap-2 px-2 py-1.5 border-b border-border">
                    <div class="text-[11px] text-muted font-medium whitespace-nowrap">模式</div>
                    <button
                      type="button"
                      class="inline-flex items-center gap-1 rounded p-1 text-muted hover:bg-hover hover:text-foreground cursor-pointer transition-colors"
                      title="模型设置"
                      @click="openAgentSettings"
                    >
                      <Settings2 class="w-3.5 h-3.5" />
                    </button>
                  </div>
                  <div class="p-1 space-y-0.5">
                    <button
                      v-for="m in PERFORMANCE_MODE_OPTIONS"
                      :key="m.value"
                      type="button"
                      class="composer-dropdown-item composer-dropdown-item--compact cursor-pointer"
                      :class="m.value === performanceMode ? 'composer-dropdown-item-active' : ''"
                      @click="selectMode(m.value)"
                    >
                      <component :is="PERFORMANCE_MODE_ICONS[m.value]" class="w-3 h-3 shrink-0" />
                      <span class="flex-1 whitespace-nowrap">{{ m.label }}</span>
                      <Check v-if="m.value === performanceMode" class="h-3 w-3 shrink-0 text-muted" />
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <button
              v-if="generating"
              class="h-9 w-9 shrink-0 rounded-xl bg-danger/20 hover:bg-danger/30 text-danger flex items-center justify-center cursor-pointer transition md:h-10 md:w-10"
              @click="chat.stop()"
              title="停止当前任务"
            ><Square class="w-4 h-4" /></button>
            <button
              class="h-9 w-9 shrink-0 rounded-xl flex items-center justify-center transition md:h-10 md:w-10"
              :class="canSend
                ? 'bg-accent text-accent-foreground hover:opacity-90 cursor-pointer'
                : 'bg-hover text-muted cursor-not-allowed'"
              :disabled="!canSend"
              :title="generating
                ? (isMacOs
                  ? '加入发送队列 (↩)。空输入再 ↩ 立即发送队首。⌘↩ 停止并立即发送'
                  : '加入发送队列 (Enter)。空输入再 Enter 立即发送队首。Ctrl+Enter 停止并立即发送')
                : (isMacOs ? '发送 (↩)' : '发送 (Enter)')"
              @click="send"
            ><Send class="w-4 h-4" /></button>
          </div>
        </div>
      </div>



      <div
        v-if="!settings.settings.hasKey"
        class="flex flex-wrap items-center gap-2 mt-2"
      >
        <div class="flex-1" />

        <span v-if="!settings.settings.hasKey" class="text-[10px] text-muted">未配置 Key</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.composer-project-dropdown {
  width: min(200px, calc(100vw - 2rem));
}
</style>
