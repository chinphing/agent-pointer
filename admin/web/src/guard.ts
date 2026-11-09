import { session } from './store'

/** 路由守卫需要的最小结构，单独抽出来才能脱离组件树单测。 */
export interface GuardTarget {
  meta: { public?: boolean }
}

export type GuardResult = true | { path: string }

/**
 * 未登录跳登录页。
 *
 * 注意 `!session.ready` 时要放行：首屏还没拿到 `/me` 的响应，
 * 这时候跳登录页会把已登录用户弹出去。
 */
export function authGuard(to: GuardTarget): GuardResult {
  if (to.meta.public) return true
  if (!session.ready) return true
  if (!session.user) return { path: '/login' }
  return true
}
