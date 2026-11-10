import { openExternalUrl } from './openExternalUrl'

/**
 * 构建期内联的控制面地址（见 `vite.config.ts` 的 `define`）。
 * 纯 Web 构建没有 Tauri IPC，只能靠这个值；桌面端不读它。
 */
declare const __POINTER_API_BASE__: string

/**
 * 控制面地址（部署配置，不是用户设置）。
 *
 * 只有一个来源：`POINTER_API_BASE`。
 * - 桌面端：`get_control_plane_base` 从 Rust 侧读回来（`build.rs` 烘焙或启动时环境变量）
 * - 纯 Web：vite 在构建期把同一个变量内联成 `__POINTER_API_BASE__`
 *
 * 没有第二个地址来源，所以不会再出现「接口走 A、页面跳 B」。
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
  // 纯 Web 构建：没有 IPC 桥，用构建期内联的同一个 POINTER_API_BASE
  return typeof __POINTER_API_BASE__ === 'string' ? __POINTER_API_BASE__ : ''
}

/**
 * 从后端读一次已保存的控制面地址。启动时调用。
 *
 * 纯 Web 构建没有 Tauri 注入的 IPC 桥，`invoke` 会直接
 * `TypeError: Cannot read properties of undefined (reading 'invoke')`。
 * 这里显式判掉，而不是用 try/catch 兜——那样连桌面端的真实故障
 * （命令没注册、IPC 失败）也会被一起吞掉，表现和「没配置」一模一样。
 */
export async function initPlatformWebBase(): Promise<void> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  const { invoke } = await import('@tauri-apps/api/core')
  const saved = await invoke<string>('get_control_plane_base')
  if (saved) setPlatformWebBase(saved)
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
  if (!base) return ''
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
