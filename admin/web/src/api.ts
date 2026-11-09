export class ApiError extends Error {
  status: number
  code?: string
  constructor(status: number, message: string, code?: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

async function request(method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(path, {
    method,
    credentials: 'include',
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const text = await res.text()
  let data: any = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = { message: text }
    }
  }
  if (!res.ok) {
    throw new ApiError(res.status, data?.message || `${t('common.requestFailed', { status: res.status })}`, data?.error)
  }
  return data
}

import { t } from './i18n'

/** multipart 上传（安装包 + 签名）。不能走 JSON 分支，浏览器要自己带 boundary。 */
async function upload(path: string, form: FormData): Promise<any> {
  const res = await fetch(path, { method: 'POST', credentials: 'include', body: form })
  const text = await res.text()
  let data: any = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = { message: text }
    }
  }
  if (!res.ok) {
    throw new ApiError(res.status, data?.message || `${t('common.requestFailed', { status: res.status })}`, data?.error)
  }
  return data
}

export const api = {
  get: (p: string) => request('GET', p),
  post: (p: string, b?: unknown) => request('POST', p, b),
  put: (p: string, b?: unknown) => request('PUT', p, b),
  del: (p: string) => request('DELETE', p),
  upload,
}

export function errText(e: unknown): string {
  if (e instanceof ApiError) return e.message
  if (e instanceof Error) return e.message
  return String(e)
}
