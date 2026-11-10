import { openExternalUrl } from './openExternalUrl'

function viteEnv(name: string): string {
  if (typeof import.meta === 'undefined') return ''
  const env = (import.meta as ImportMeta & { env?: Record<string, string> }).env
  return String(env?.[name] ?? '').trim()
}

/**
 * 控制面地址（部署配置，不是用户设置）。
 *
 * 来源只有构建期注入的 `POINTER_API_BASE`（managed 构建）或启动时的同名环境变量。
 * 启动时由 `initPlatformWebBase()` 从后端读回来，前端据此拼账单页/下载页路径。
 * 构建期 `VITE_POINTER_WEB_BASE` 只作为非 Tauri 环境（纯 Web 构建）的兜底。
 */
let runtimeWebBase = ''

/** 设置运行期控制面地址（去掉尾部斜杠）。 */
export function setPlatformWebBase(base: string): void {
  runtimeWebBase = String(base ?? '')
    .trim()
    .replace(/\/+$/, '')
}

/** 当前生效的控制面地址；未绑定时为空串，调用方保持静默。 */
export function platformWebBase(): string {
  if (runtimeWebBase) return runtimeWebBase
  return viteEnv('VITE_POINTER_WEB_BASE').replace(/\/+$/, '')
}

/**
 * 从后端读一次已保存的控制面地址。启动时调用；失败不影响其他功能。
 */
export async function initPlatformWebBase(): Promise<void> {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const saved = await invoke<string>('get_control_plane_base')
    if (saved) setPlatformWebBase(saved)
  } catch {
    // 浏览器 / 非 Tauri 环境：保持构建期兜底
  }
}

/** 账单/充值页。控制面与官网用同一个路径，客户端不需要按部署分支。 */
export function platformBillingUrl(): string {
  const base = platformWebBase()
  if (!base) return ''
  return `${base}/me/billing`
}

/** 下载页。同样两边同路径。 */
export function platformDownloadUrl(): string {
  const base = platformWebBase()
  if (!base) return viteEnv('VITE_POINTER_DOWNLOAD_URL')
  return `${base}/download`
}

export async function openPlatformBillingPage(): Promise<void> {
  const url = platformBillingUrl()
  if (!url) {
    console.warn('platformUrls: billing page skipped (no web base in this edition)')
    return
  }
  await openExternalUrl(url)
}

export function isBalanceExhaustedMessage(text: string | null | undefined): boolean {
  const t = (text ?? '').trim()
  if (!t) return false
  return (
    t.includes('token_quota_exhausted') ||
    t.includes('账户余额已用尽') ||
    t.includes('账户余额不足')
  )
}
