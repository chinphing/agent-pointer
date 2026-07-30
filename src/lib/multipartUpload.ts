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
    const xhr = new XMLHttpRequest()
    xhr.open('POST', url)
    xhr.withCredentials = true
    let timeoutId: number | undefined
    if (options.timeoutMs != null && options.timeoutMs > 0) {
      timeoutId = window.setTimeout(() => {
        xhr.abort()
        reject(new Error(`上传超时（>${Math.round(options.timeoutMs! / 1000)}s）`))
      }, options.timeoutMs)
    }
    const clearTimer = () => {
      if (timeoutId !== undefined) window.clearTimeout(timeoutId)
    }
    xhr.upload.onprogress = ev => {
      if (!ev.lengthComputable || !options.onProgress) return
      const percent = Math.min(100, Math.round((ev.loaded / ev.total) * 100))
      options.onProgress({ loaded: ev.loaded, total: ev.total, percent })
    }
    xhr.onload = () => {
      clearTimer()
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(xhr.responseText || `上传失败 (${xhr.status})`))
        return
      }
      if (!xhr.responseText?.trim()) {
        resolve(undefined as T)
        return
      }
      try {
        resolve(JSON.parse(xhr.responseText) as T)
      } catch (e) {
        reject(e instanceof Error ? e : new Error('解析上传响应失败'))
      }
    }
    xhr.onerror = () => {
      clearTimer()
      reject(new Error('网络错误，上传失败'))
    }
    xhr.onabort = () => {
      clearTimer()
      reject(new Error('上传已取消'))
    }
    xhr.send(form)
  })
}
