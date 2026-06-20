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
  return '视频上传失败'
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
  onProgress: (p: VideoOssProgress) => void
): Promise<VideoOssUploadResult> {
  const { invoke } = await import('@tauri-apps/api/core')
  const { listen } = await import('@tauri-apps/api/event')
  const unlisten = await listen<VideoOssProgress>('composer-video-oss-progress', ev => {
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
  onProgress: (p: VideoOssProgress) => void
): Promise<VideoOssUploadResult> {
  const form = new FormData()
  if (conversationId?.trim()) form.append('conversationId', conversationId.trim())
  form.append('attachmentId', attachmentId)
  form.append('fileName', file.name)
  form.append('mimeType', file.type || 'video/mp4')
  form.append('compress', compress ? 'true' : 'false')
  form.append('file', file)

  return await new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('POST', '/api/chat/upload-video-oss')
    xhr.upload.onprogress = ev => {
      if (!ev.lengthComputable) return
      const percent = Math.min(100, Math.round((ev.loaded / ev.total) * 100))
      onProgress({
        attachmentId,
        loaded: ev.loaded,
        total: ev.total,
        percent
      })
    }
    xhr.onload = () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(xhr.responseText || `上传失败 (${xhr.status})`))
        return
      }
      try {
        const parsed = JSON.parse(xhr.responseText) as VideoOssUploadResult
        resolve(parsed)
      } catch (e) {
        reject(e instanceof Error ? e : new Error('解析上传响应失败'))
      }
    }
    xhr.onerror = () => reject(new Error('网络错误，视频上传失败'))
    xhr.send(form)
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
        onProgress
      )
    }
    throw new Error(
      '请使用附件按钮（回形针）选择视频文件。桌面端不支持通过网页式文件选择器上传大视频。'
    )
  }
  return await uploadViaWebApi(attachmentId, conversationId, file, compress, onProgress)
}
