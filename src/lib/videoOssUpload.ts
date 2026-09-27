import { t } from '../i18n'
import type { ModelSettings } from '../types/chat'
import { isTauriRuntime } from './runtime'

export interface VideoOssUploadResult {
  remoteUrl: string
  ossObjectKey: string
  storageRelPath?: string
}

export type VideoOssProgress = {
  attachmentId: string
  loaded: number
  total: number
  percent: number
}

export type VideoOssUploadOptions = {
  compress?: boolean
  conversationId?: string
  signal?: AbortSignal
}

export function isMediaOssConfigured(settings: ModelSettings): boolean {
  const o = settings.mediaOss
  if (!o?.enabled) return false
  const hasCreds = !!(o.accessKeyId?.trim() && o.accessKeySecret?.trim())
  if (!hasCreds) return false
  return !!(
    (o.bucket?.trim() && o.region?.trim()) ||
    o.endpoint?.trim()
  )
}

/** Tauri invoke rejects with a plain string — not an Error instance. */
export function formatVideoOssInvokeError(err: unknown): string {
  if (typeof err === 'string' && err.trim()) return err.trim()
  if (err instanceof Error && err.message.trim()) return err.message.trim()
  if (err && typeof err === 'object' && 'message' in err) {
    const msg = String((err as { message: unknown }).message).trim()
    if (msg) return msg
  }
  console.error('video OSS upload failed with unknown error', err)
  return t('upload.videoFailed')
}

export interface MediaOssUploadStatus {
  configured: boolean
  message?: string
}

export async function getMediaOssUploadStatus(): Promise<MediaOssUploadStatus> {
  if (!isTauriRuntime()) {
    return { configured: false }
  }
  const { invoke } = await import('@tauri-apps/api/core')
  return await invoke<MediaOssUploadStatus>('get_media_oss_upload_status')
}

async function uploadViaTauriPath(
  attachmentId: string,
  conversationId: string | undefined,
  path: string,
  fileName: string,
  mimeType: string,
  compress: boolean,
  onProgress: (p: VideoOssProgress) => void,
  signal?: AbortSignal
): Promise<VideoOssUploadResult> {
  if (signal?.aborted) {
    throw new Error(t('upload.aborted'))
  }
  const { invoke } = await import('@tauri-apps/api/core')
  const { listen } = await import('@tauri-apps/api/event')
  const unlisten = await listen<VideoOssProgress>('composer-video-oss-progress', ev => {
    if (signal?.aborted) return
    if (ev.payload.attachmentId === attachmentId) {
      onProgress(ev.payload)
    }
  })
  try {
    const result = await invoke<VideoOssUploadResult>('upload_composer_video_to_oss', {
      conversationId: conversationId ?? '',
      attachmentId,
      path,
      fileName,
      mimeType,
      compress
    })
    if (signal?.aborted) {
      throw new Error(t('upload.aborted'))
    }
    return {
      remoteUrl: result.remoteUrl,
      ossObjectKey: result.ossObjectKey,
      storageRelPath: result.storageRelPath
    }
  } finally {
    await unlisten()
  }
}

async function uploadViaWebApi(
  attachmentId: string,
  conversationId: string | undefined,
  file: File,
  compress: boolean,
  onProgress: (p: VideoOssProgress) => void,
  signal?: AbortSignal
): Promise<VideoOssUploadResult> {
  const form = new FormData()
  if (conversationId?.trim()) form.append('conversationId', conversationId.trim())
  form.append('attachmentId', attachmentId)
  form.append('fileName', file.name)
  form.append('mimeType', file.type || 'video/mp4')
  form.append('compress', compress ? 'true' : 'false')
  form.append('file', file)

  const { postMultipartJson } = await import('./multipartUpload')
  return await postMultipartJson<VideoOssUploadResult>('/api/chat/upload-video-oss', form, {
    signal,
    onProgress: p =>
      onProgress({
        attachmentId,
        loaded: p.loaded,
        total: p.total,
        percent: p.percent
      })
  })
}

export async function uploadComposerVideoToOss(
  attachmentId: string,
  file: File,
  localPath: string | undefined,
  onProgress: (p: VideoOssProgress) => void,
  options: VideoOssUploadOptions = {}
): Promise<VideoOssUploadResult> {
  const compress = options.compress === true
  const conversationId = options.conversationId
  const signal = options.signal
  if (signal?.aborted) {
    throw new Error(t('upload.aborted'))
  }
  onProgress({ attachmentId, loaded: 0, total: file.size, percent: 0 })
  if (isTauriRuntime()) {
    if (localPath?.trim()) {
      return await uploadViaTauriPath(
        attachmentId,
        conversationId,
        localPath,
        file.name,
        file.type || 'video/mp4',
        compress,
        onProgress,
        signal
      )
    }
    throw new Error(
      t('attachment.usePaperclipForVideo')
    )
  }
  return await uploadViaWebApi(attachmentId, conversationId, file, compress, onProgress, signal)
}
