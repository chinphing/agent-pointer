import { t } from '../i18n'
import { WEB_API_BASE } from './runtime'

export type MultipartUploadProgress = {
  loaded: number
  total: number
  percent: number
}

export type MultipartUploadOptions = {
  /** Abort after this many ms (default: no client abort). */
  timeoutMs?: number
  onProgress?: (p: MultipartUploadProgress) => void
  /** Abort in-flight XHR (user cancel / remove chip). */
  signal?: AbortSignal
}

/** Stable internal code for abort detection across locales. */
export const UPLOAD_ABORTED_CODE = 'upload_aborted'

/** Localized user-facing abort copy (also thrown as Error.message for display). */
export function uploadAbortedMessage(): string {
  return t('errors.uploadAborted')
}

/**
 * @deprecated Prefer uploadAbortedMessage() for display.
 * Kept as the stable throw/sentinel string; isUploadAbortedError recognizes it.
 */
export const UPLOAD_ABORTED_MESSAGE = UPLOAD_ABORTED_CODE

export function isUploadAbortedError(err: unknown): boolean {
  const msg = (err instanceof Error ? err.message : String(err)).trim()
  return (
    msg === UPLOAD_ABORTED_CODE ||
    msg === t('errors.uploadAborted') ||
    msg === '上传已取消' ||
    msg === 'Upload cancelled' ||
    /upload aborted|aborted/i.test(msg)
  )
}

/**
 * Map wire transfer bytes → UI percent.
 * 100% means bytes have left the browser; the HTTP response (server save / OSS)
 * may still be pending — chips should label that as processing, not “done”.
 */
export function mapUploadTransferPercent(loaded: number, total: number): number {
  if (!Number.isFinite(loaded) || !Number.isFinite(total) || total <= 0) return 0
  return Math.min(100, Math.max(0, Math.round((Math.max(0, loaded) / total) * 100)))
}

/**
 * POST multipart/form-data and parse JSON response.
 * Uses XHR so upload progress events are available (fetch cannot).
 */
export function postMultipartJson<T>(
  path: string,
  form: FormData,
  options: MultipartUploadOptions = {}
): Promise<T> {
  const url = path.startsWith('http') ? path : `${WEB_API_BASE}${path}`
  return new Promise((resolve, reject) => {
    if (options.signal?.aborted) {
      reject(new Error(UPLOAD_ABORTED_CODE))
      return
    }
    const xhr = new XMLHttpRequest()
    xhr.open('POST', url)
    xhr.withCredentials = true
    let timeoutId: number | undefined
    if (options.timeoutMs != null && options.timeoutMs > 0) {
      timeoutId = window.setTimeout(() => {
        xhr.abort()
        reject(
          new Error(
            t('errors.uploadTimeout', {
              seconds: Math.round(options.timeoutMs! / 1000)
            })
          )
        )
      }, options.timeoutMs)
    }
    const clearTimer = () => {
      if (timeoutId !== undefined) window.clearTimeout(timeoutId)
    }
    const onAbortSignal = () => {
      clearTimer()
      xhr.abort()
    }
    options.signal?.addEventListener('abort', onAbortSignal, { once: true })
    const cleanupSignal = () => {
      options.signal?.removeEventListener('abort', onAbortSignal)
    }
    let lastLoaded = 0
    let lastTotal = 0
    const reportTransfer = (loaded: number, total: number) => {
      if (!options.onProgress) return
      lastLoaded = loaded
      lastTotal = total
      options.onProgress({
        loaded,
        total,
        percent: mapUploadTransferPercent(loaded, total)
      })
    }
    xhr.upload.onprogress = ev => {
      if (!ev.lengthComputable) return
      reportTransfer(ev.loaded, ev.total)
    }
    // Bytes fully sent; UI stays at 100% while waiting for the response body.
    xhr.upload.onload = () => {
      if (lastTotal > 0) {
        reportTransfer(lastTotal, lastTotal)
      } else {
        reportTransfer(1, 1)
      }
    }
    xhr.onload = () => {
      clearTimer()
      cleanupSignal()
      if (options.signal?.aborted) {
        reject(new Error(UPLOAD_ABORTED_CODE))
        return
      }
      if (xhr.status < 200 || xhr.status >= 300) {
        const body = (xhr.responseText || '').trim()
        if (body.includes('platform_login_required') || xhr.status === 401) {
          reject(new Error('platform_login_required'))
          return
        }
        if (body.includes('local_login_required')) {
          reject(new Error('local_login_required'))
          return
        }
        reject(new Error(body || t('errors.uploadFailed', { status: xhr.status })))
        return
      }
      if (!xhr.responseText?.trim()) {
        resolve(undefined as T)
        return
      }
      try {
        resolve(JSON.parse(xhr.responseText) as T)
      } catch (e) {
        reject(e instanceof Error ? e : new Error(t('errors.uploadParseFailed')))
      }
    }
    xhr.onerror = () => {
      clearTimer()
      cleanupSignal()
      reject(new Error(t('errors.uploadNetworkFailed')))
    }
    xhr.onabort = () => {
      clearTimer()
      cleanupSignal()
      reject(new Error(UPLOAD_ABORTED_CODE))
    }
    xhr.send(form)
  })
}
