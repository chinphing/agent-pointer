import { beforeEach, describe, expect, it } from 'vitest'
import { authGuard } from './guard'
import { session } from './store'

const protectedRoute = { meta: {} }
const publicRoute = { meta: { public: true } }

describe('authGuard', () => {
  beforeEach(() => {
    session.ready = false
    session.user = null
    session.permissions = []
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

  it('未登录访问具体页面 → 带上 ?next= 记住原目标', () => {
    session.ready = true
    session.user = null
    expect(authGuard({ meta: {}, fullPath: '/me/billing' })).toEqual({
      path: '/login',
      query: { next: '/me/billing' },
    })
  })

  it('原目标带查询串时完整保留', () => {
    session.ready = true
    session.user = null
    expect(authGuard({ meta: {}, fullPath: '/admin/members?q=zhang' })).toEqual({
      path: '/login',
      query: { next: '/admin/members?q=zhang' },
    })
  })

  it('原目标本身是登录页时不再嵌套 ?next=', () => {
    session.ready = true
    session.user = null
    expect(authGuard({ meta: {}, fullPath: '/login' })).toEqual({ path: '/login' })
  })

  it('原目标不是站内路径时丢弃，避免开放重定向', () => {
    session.ready = true
    session.user = null
    expect(authGuard({ meta: {}, fullPath: '//evil.example.com/x' })).toEqual({ path: '/login' })
  })

  it('已登录 → 放行', () => {
    session.ready = true
    session.user = { id: 'u1', username: 'admin', displayName: '管理员' }
    expect(authGuard(protectedRoute)).toBe(true)
  })

  it('有权限的页面放行', () => {
    session.ready = true
    session.user = { id: 'u1', username: 'admin', displayName: '管理员' }
    session.permissions = ['audit:read']
    expect(authGuard({ meta: { perm: 'audit:read' } })).toBe(true)
  })

  it('没权限的页面重定向到能看的落地页，而不是渲染后吃 403', () => {
    session.ready = true
    session.user = { id: 'u2', username: 'member', displayName: '成员' }
    session.permissions = []
    // 零权限用户的落地页是 /me（登录即可）
    expect(authGuard({ meta: { perm: 'audit:read' } })).toEqual({ path: '/me' })
  })

  it('任一权限命中即可（资源页跨多种 kind）', () => {
    session.ready = true
    session.user = { id: 'u3', username: 'ops', displayName: '运营' }
    session.permissions = ['model:read']
    expect(
      authGuard({ meta: { perm: ['provider:read', 'model:read', 'skill:read'] } })
    ).toBe(true)
  })
})
