import { session } from './store'
import { canView, landingFor } from './routes'

/** 路由守卫需要的最小结构，单独抽出来才能脱离组件树单测。 */
export interface GuardTarget {
  meta: { public?: boolean; perm?: string | string[] }
  fullPath?: string
}

export type GuardResult = true | { path: string; query?: Record<string, string> }

/**
 * 未登录跳登录页；登录了但没这条路由的权限，跳到他**能看**的第一页。
 *
 * 注意 `!session.ready` 时要放行：首屏还没拿到 `/me` 的响应，
 * 这时候跳登录页会把已登录用户弹出去。
 *
 * 权限不足不渲染页面再吃 403 —— 那会让用户停在一个红框上，
 * 而真正的原因是他本来就不该看到这个页面。
 *
 * 未登录时把**原本要去的路径**带进 `?next=`：客户端「余额」直接在浏览器新标签打开
 * `/me/billing`，那个标签没有会话，登录完必须回到账单页，而不是被扔到角色首页。
 */
export function authGuard(to: GuardTarget): GuardResult {
  if (to.meta.public) return true
  if (!session.ready) return true
  if (!session.user) {
    const target = to.fullPath?.trim()
    if (target && target !== '/login' && target.startsWith('/') && !target.startsWith('//')) {
      return { path: '/login', query: { next: target } }
    }
    return { path: '/login' }
  }
  if (!canView(to.meta.perm, session.permissions)) {
    return { path: landingFor(session.permissions) }
  }
  return true
}
