import { beforeEach, describe, expect, it } from 'vitest'
import { authGuard } from './guard'
import { session } from './store'

const protectedRoute = { meta: {} }
const publicRoute = { meta: { public: true } }

describe('authGuard', () => {
  beforeEach(() => {
    session.ready = false
    session.user = null
  })

  it('公开页（登录页）直接放行', () => {
    session.ready = true
    expect(authGuard(publicRoute)).toBe(true)
  })

  it('会话还没加载完时放行，避免把已登录用户弹出去', () => {
    session.ready = false
    session.user = null
    expect(authGuard(protectedRoute)).toBe(true)
  })

  it('加载完且没有用户 → 跳登录页', () => {
    session.ready = true
    session.user = null
    expect(authGuard(protectedRoute)).toEqual({ path: '/login' })
  })

  it('已登录 → 放行', () => {
    session.ready = true
    session.user = { id: 'u1', username: 'admin', displayName: '管理员' }
    expect(authGuard(protectedRoute)).toBe(true)
  })
})
