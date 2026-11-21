import { convertFileSrc } from '@tauri-apps/api/core'
import { isUsableAttachmentPreviewUrl } from './attachmentSupport'
import { previewChatMedia } from './api'
import { isTauriRuntime } from './runtime'
import { getChatMediaLocalPath } from './tauri'
import { chatMediaStreamObjectUrl } from './web'

/** Desktop: streamable file URL for a saved conversation-media video. Web: same-origin stream endpoint. */
export async function videoPreviewUrlFromStorage(
  storageRelPath: string,
  conversationId?: string | null
): Promise<string | null> {
  if (!storageRelPath.trim()) return null
  if (!isTauriRuntime()) {
    return chatMediaStreamObjectUrl(storageRelPath.trim(), conversationId)
  }
  try {
    const abs = await getChatMediaLocalPath(storageRelPath.trim(), conversationId)
    if (!abs.trim()) return null
    return convertFileSrc(abs)
  } catch (err) {
    if (conversationId?.trim()) {
      try {
        const preview = await previewChatMedia(storageRelPath.trim(), conversationId)
        const mime = preview.mimeType?.trim() || 'application/octet-stream'
        return `data:${mime};base64,${preview.dataBase64}`
      } catch (previewErr) {
        console.warn('videoPreviewUrlFromStorage sandbox read failed', previewErr)
      }
    }
    console.warn('videoPreviewUrlFromStorage failed', err)
    return null
  }
}

export interface VideoPreviewSource {
  kind: string
  storageRelPath?: string
  remoteUrl?: string
  previewUrl?: string
}

/** Local file (desktop) or HTTPS OSS URL for inline video preview. */
export async function resolveVideoPreviewUrl(
  att: VideoPreviewSource,
  conversationId?: string | null
): Promise<string | null> {
  if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
  if (att.kind !== 'video') return null
  if (att.storageRelPath?.trim()) {
    const local = await videoPreviewUrlFromStorage(att.storageRelPath, conversationId)
    if (local) return local
  }
  const remote = att.remoteUrl?.trim()
  if (remote?.startsWith('http://') || remote?.startsWith('https://')) return remote
  return null
}

/** Desktop: preview URL from an absolute local path (before backup is saved). */
export async function videoPreviewUrlFromLocalPath(
  localPath: string
): Promise<string | null> {
  if (!localPath.trim() || !isTauriRuntime()) return null
  try {
    return convertFileSrc(localPath.trim())
  } catch (err) {
    console.warn('videoPreviewUrlFromLocalPath failed', err)
    return null
  }
}
