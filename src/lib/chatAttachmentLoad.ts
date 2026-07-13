import { convertFileSrc } from '@tauri-apps/api/core'
import type { RenderableAttachment } from './messageNormalizer'
import { previewChatMedia, previewMediaRef } from './api'
import { resolveAttachmentAbsPath } from './attachmentLocalPath'
import { isUsableAttachmentPreviewUrl } from './attachmentSupport'
import {
  resolveVideoPreviewUrl,
  videoPreviewUrlFromLocalPath
} from './chatMediaPreview'
import { isTauriRuntime } from './runtime'

/** Only images are eagerly preview-loaded in chat bubbles. */
export function shouldEagerLoadPreview(att: Pick<RenderableAttachment, 'kind'>): boolean {
  if (att.kind === 'image') return true
  return isTauriRuntime() && (att.kind === 'video' || att.kind === 'audio')
}

/** Web: non-image attachments show a download action instead of inline media. */
export function showsWebDownloadOnly(att: Pick<RenderableAttachment, 'kind'>): boolean {
  return !isTauriRuntime() && att.kind !== 'image'
}

export async function resolveImagePreviewDataUrl(
  att: RenderableAttachment
): Promise<string | null> {
  if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()

  let preview
  if (att.mediaRef && !att.storageRelPath) {
    preview = await previewMediaRef(att.mediaRef)
  } else if (att.localAbsPath) {
    preview = await previewMediaRef(att.localAbsPath)
  } else if (att.storageRelPath) {
    preview = await previewChatMedia(att.storageRelPath)
  } else if (att.mediaRef) {
    preview = await previewMediaRef(att.mediaRef)
  } else {
    return null
  }

  const mime =
    preview.mimeType && preview.mimeType !== 'application/octet-stream'
      ? preview.mimeType
      : att.mimeType || preview.mimeType || 'application/octet-stream'
  return `data:${mime};base64,${preview.dataBase64}`
}

/** Desktop local file URL for inline video/audio playback (no preview API). */
export async function resolveDesktopPlaybackUrl(
  att: RenderableAttachment
): Promise<string | null> {
  if (!isTauriRuntime()) return null

  if (att.kind === 'video') {
    const fromStorage = await resolveVideoPreviewUrl(att)
    if (fromStorage) return fromStorage
    if (att.localAbsPath?.trim()) {
      return videoPreviewUrlFromLocalPath(att.localAbsPath)
    }
    const abs = await resolveAttachmentAbsPath(att)
    if (abs) return convertFileSrc(abs)
    return null
  }

  if (att.kind === 'audio') {
    if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
    const abs = await resolveAttachmentAbsPath(att)
    if (abs) return convertFileSrc(abs)
    return null
  }

  return null
}

export async function resolveEagerPreviewUrl(
  att: RenderableAttachment
): Promise<string | null> {
  if (att.kind === 'image') return resolveImagePreviewDataUrl(att)
  if (isTauriRuntime() && (att.kind === 'video' || att.kind === 'audio')) {
    return resolveDesktopPlaybackUrl(att)
  }
  return null
}
