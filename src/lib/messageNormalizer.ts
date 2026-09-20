import { isPersistableAttachmentPreviewUrl, isUsableAttachmentPreviewUrl } from './attachmentSupport'
import type { ChatMessage, MediaAttachment, MediaAttachmentKind } from '../types/chat'
import { getComposerAttachmentPreviewUrl } from './attachmentPayloadStore'
import type { ComposerAttachment } from '../types/chat'
import { extractOutboundMediaPaths, stripOutboundMediaMarkers } from './outboundMedia'

export interface RenderableAttachment {
  id: string
  kind: MediaAttachment['kind']
  fileName: string
  mimeType: string
  previewUrl?: string
  storageRelPath?: string
  localAbsPath?: string
  remoteUrl?: string
  /** Resolved preview key: storageRelPath or localAbsPath or pointer-media ref. */
  mediaRef?: string
  /** ASR / document extraction cached on attachment. */
  derivedText?: string
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function attachmentDataUrlFromBase64(att: MediaAttachment): string | undefined {
  const b64 = att.contentBase64?.trim()
  if (!b64) return undefined
  const mime = att.mimeType || 'application/octet-stream'
  return `data:${mime};base64,${b64}`
}

export function attachmentPreviewUrl(att: MediaAttachment): string | undefined {
  if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
  const fromStore = getComposerAttachmentPreviewUrl(att as ComposerAttachment)
  if (fromStore) return fromStore
  return attachmentDataUrlFromBase64(att)
}

function fileNameFromPath(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || 'attachment'
}

function stripFileUri(raw: string): string | undefined {
  const trimmed = raw.trim()
  if (!trimmed.toLowerCase().startsWith('file://')) return undefined
  const rest = trimmed.slice(7).trim()
  if (!rest) return undefined
  if (rest.startsWith('/')) {
    const without = rest.replace(/^\/+/, '')
    if (/^[A-Za-z]:/.test(without)) return without.replace(/\\/g, '/')
    return rest
  }
  return rest.replace(/\\/g, '/')
}

function normalizeMediaPath(path: string): string {
  return stripFileUri(path) ?? path.trim()
}

function mimeFromFileName(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'png') return 'image/png'
  if (ext === 'jpg' || ext === 'jpeg') return 'image/jpeg'
  if (ext === 'gif') return 'image/gif'
  if (ext === 'webp') return 'image/webp'
  if (ext === 'svg') return 'image/svg+xml'
  if (ext === 'pdf') return 'application/pdf'
  if (ext === 'html' || ext === 'htm') return 'text/html'
  if (ext === 'json') return 'application/json'
  if (ext === 'csv') return 'text/csv'
  if (ext === 'txt' || ext === 'md') return 'text/plain'
  if (ext === 'mp4' || ext === 'm4v') return 'video/mp4'
  if (ext === 'webm') return 'video/webm'
  if (ext === 'mov') return 'video/quicktime'
  if (ext === 'mkv') return 'video/x-matroska'
  if (ext === 'mp3') return 'audio/mpeg'
  if (ext === 'wav') return 'audio/wav'
  if (ext === 'm4a') return 'audio/mp4'
  return 'application/octet-stream'
}

function kindFromFileName(fileName: string): MediaAttachmentKind {
  const ext = fileName.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(ext)) return 'image'
  if (['mp4', 'webm', 'mov', 'mkv'].includes(ext)) return 'video'
  if (['mp3', 'wav', 'm4a', 'aac', 'ogg', 'flac'].includes(ext)) return 'audio'
  if (['pdf', 'txt', 'md', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'html', 'htm', 'json', 'csv', 'svg'].includes(ext)) return 'document'
  return 'file'
}

function normalizeDisplayFileName(fileName: string): string {
  const trimmed = fileName.trim()
  if (!trimmed.includes('%')) return trimmed
  try {
    return decodeURIComponent(trimmed.replace(/\+/g, ' '))
  } catch {
    return trimmed
  }
}

function renderableFromMediaAttachment(att: MediaAttachment): RenderableAttachment {
  const mediaRef = att.localAbsPath ?? att.storageRelPath
  return {
    id: att.id,
    kind: att.kind,
    fileName: normalizeDisplayFileName(att.fileName),
    mimeType: att.mimeType,
    storageRelPath: att.storageRelPath,
    localAbsPath: att.localAbsPath,
    remoteUrl: att.remoteUrl,
    mediaRef,
    previewUrl: attachmentPreviewUrl(att),
    derivedText: att.derivedText
  }
}

/** Model-only path hints injected into user content; never show in the chat UI. */
export function stripSavedAttachmentHints(content: string): string {
  return content
    .replace(
      /Saved attachment:\s*\n- URI:\s*[^\n]+(?:\n- Local path:\s*[^\n]+)?(?:\n[^\n]*(?:file_read|Skill|terminal)[^\n]*)?/gi,
      ''
    )
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

const IDLE_JOB_PUSH_USER_LINE = '后台任务已完成。'
const IDLE_JOB_PUSH_HOST_KIND = 'idle_job_push'
const IDLE_JOB_PUSH_BUBBLE_TITLE_CHARS = 20
const IDLE_JOB_PUSH_BUBBLE_TITLE_MAX = 2
const IDLE_JOB_PUSH_JOB_HEADING_RE =
  /^###[ \t]+(.+)\nkind:[ \t]+\S+\nstatus:[ \t]+(\S+)\njobId:/gm

type IdleJobPushHeading = { title: string; failed: boolean }

function truncateIdleJobPushTitle(text: string): string {
  const chars = Array.from(text.trim())
  if (chars.length <= IDLE_JOB_PUSH_BUBBLE_TITLE_CHARS) return chars.join('')
  return `${chars.slice(0, IDLE_JOB_PUSH_BUBBLE_TITLE_CHARS - 1).join('')}…`
}

function parseIdleJobPushHeadings(content: string): IdleJobPushHeading[] {
  const out: IdleJobPushHeading[] = []
  const re = new RegExp(IDLE_JOB_PUSH_JOB_HEADING_RE.source, 'gm')
  let match: RegExpExecArray | null
  while ((match = re.exec(content)) !== null) {
    const title = match[1]?.trim() ?? ''
    if (!title) continue
    out.push({ title, failed: match[2] === 'failed' })
  }
  return out
}

function formatIdleJobPushBubble(jobs: IdleJobPushHeading[]): string {
  if (!jobs.length) return IDLE_JOB_PUSH_USER_LINE
  const allFailed = jobs.every(job => job.failed)
  const prefix = allFailed ? '后台任务失败' : '后台任务已完成'
  const titles = jobs.map(job => truncateIdleJobPushTitle(job.title))
  if (titles.length <= IDLE_JOB_PUSH_BUBBLE_TITLE_MAX) {
    return `${prefix}：${titles.join('、')}`
  }
  return `${prefix}：${titles[0]}、${titles[1]} 等 ${titles.length} 个`
}

function idleJobPushDisplayLine(message: ChatMessage): string {
  const bubble = message.uiBindings?.bubbleText?.trim() ?? ''
  if (bubble && bubble !== IDLE_JOB_PUSH_USER_LINE) return bubble
  const jobs = parseIdleJobPushHeadings(message.content ?? '')
  if (jobs.length) return formatIdleJobPushBubble(jobs)
  return bubble || IDLE_JOB_PUSH_USER_LINE
}

/** Hide model injection blocks from the user bubble (ASR under player, path hints, etc.). */
export function userMessageDisplayContent(message: ChatMessage): string {
  if (message.uiBindings?.hostKind === IDLE_JOB_PUSH_HOST_KIND) {
    return idleJobPushDisplayLine(message)
  }
  const bubble = message.uiBindings?.bubbleText?.trim()
  if (bubble) return bubble
  let content = stripSavedAttachmentHints(message.content?.trim() ?? '')
  content = stripOutboundMediaMarkers(content)
  if (!content) return ''
  const attachments = message.attachments ?? []
  const audioAtts = attachments.filter(
    a => a.kind === 'audio' || a.mimeType?.toLowerCase().startsWith('audio/')
  )
  if (!audioAtts.length) return content
  for (const att of audioAtts) {
    const derived = att.derivedText?.trim()
    if (derived) {
      const blockRe = new RegExp(
        `\\[Audio:\\s*${escapeRegExp(att.fileName)}\\]\\s*\\n?\\s*${escapeRegExp(derived)}`,
        'gi'
      )
      content = content.replace(blockRe, '').trim()
      if (content === derived) content = ''
    } else {
      content = content
        .replace(new RegExp(`\\[Audio:\\s*${escapeRegExp(att.fileName)}\\]\\s*`, 'gi'), '')
        .trim()
    }
  }
  content = content.replace(/\[Audio:[^\]]+\]\s*/gi, '').trim()
  return stripSavedAttachmentHints(content)
}

function isUserFilesystemPath(path: string): boolean {
  const t = path.trim()
  if (stripFileUri(t)) return true
  if (t === '~' || t.startsWith('~/') || t.startsWith('~\\')) return true
  if (/^[A-Za-z]:[\\/]/.test(t)) return true
  return path.startsWith('/')
}

function renderableFromMediaPath(path: string, index: number): RenderableAttachment {
  const normalized = normalizeMediaPath(path)
  const fileName = fileNameFromPath(normalized)
  const isFs = isUserFilesystemPath(normalized)
  const storageRelPath = !isFs && normalized.includes('/') ? normalized : undefined
  const localAbsPath = isFs ? normalized : undefined
  return {
    id: `reply-media-draft-${index}`,
    kind: kindFromFileName(fileName),
    fileName,
    mimeType: mimeFromFileName(fileName),
    storageRelPath,
    localAbsPath,
    mediaRef: normalized
  }
}

export function attachmentsForMessageRender(message: ChatMessage): RenderableAttachment[] {
  const list = message.attachments ?? []
  if (list.length) return list.map(renderableFromMediaAttachment)
  return []
}

/** Assistant bubble: only use persisted attachments. */
export function assistantReplyMediaForRender(
  message: ChatMessage | undefined
): RenderableAttachment[] {
  if (message?.attachments?.length) {
    return message.attachments.map(renderableFromMediaAttachment)
  }
  return []
}

export function stripWireAttachmentFields(messages: ChatMessage[]): ChatMessage[] {
  return messages.map(msg => {
    if (!msg.attachments?.length) return msg
    return {
      ...msg,
      attachments: msg.attachments.map(({ contentBase64, previewUrl, ...rest }) => {
        const keepBase64 =
          contentBase64?.trim() &&
          !rest.storageRelPath?.trim() &&
          (rest.kind === 'audio' || rest.mimeType?.toLowerCase().startsWith('audio/'))
        return {
          ...rest,
          ...(keepBase64 ? { contentBase64: contentBase64!.trim() } : {}),
          // data: URLs are UI-only (base64 in memory + disk); only http(s) survive persist.
          ...(isPersistableAttachmentPreviewUrl(previewUrl) ? { previewUrl: previewUrl!.trim() } : {})
        }
      })
    }
  })
}
