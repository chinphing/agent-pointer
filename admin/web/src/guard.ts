import { session } from './store'
import { canView, landingFor } from './routes'

/** 路由守卫需要的最小结构，单独抽出来才能脱离组件树单测。 */
export interface GuardTarget {
  meta: { public?: boolean; perm?: string | string[] }
}

export type GuardResult = true | { path: string }

/**
 * 未登录跳登录页；登录了但没这条路由的权限，跳到他**能看**的第一页。
 *
 * 注意 `!session.ready` 时要放行：首屏还没拿到 `/me` 的响应，
 * 这时候跳登录页会把已登录用户弹出去。
 *
 * 权限不足不渲染页面再吃 403 —— 那会让用户停在一个红框上，
 * 而真正的原因是他本来就不该看到这个页面。
 */
export function authGuard(to: GuardTarget): GuardResult {
  if (to.meta.public) return true
  if (!session.ready) return true
  if (!session.user) return { path: '/login' }
  if (!canView(to.meta.perm, session.permissions)) {
    return { path: landingFor(session.permissions) }
  }
  return true
}
