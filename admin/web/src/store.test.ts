import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { can, canAny, loadSession, login, logout, session } from './store'

function mockMe(body: unknown, status = 200) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      new Response(JSON.stringify(body), {
        status,
        headers: { 'content-type': 'application/json' },
      })
    )
  )
}

function resetSession() {
  session.ready = false
  session.user = null
  session.roles = []
  session.permissions = []
}

describe('权限判断', () => {
  beforeEach(() => {
    resetSession()
    session.permissions = ['provider:read', 'provider:write', 'user:read']
  })

  it('can 精确匹配', () => {
    expect(can('provider:read')).toBe(true)
    expect(can('provider:delete')).toBe(false)
  })

  it('canAny 按前缀命中任一动作', () => {
    expect(canAny('provider')).toBe(true)
    expect(canAny('skill')).toBe(false)
  })

  it('前缀不会误伤相似域名', () => {
    resetSession()
    session.permissions = ['providerx:read']
    expect(canAny('provider')).toBe(false)
  })

  it('没有权限时一律 false', () => {
    resetSession()
    expect(can('provider:read')).toBe(false)
    expect(canAny('provider')).toBe(false)
  })
})

describe('loadSession', () => {
  beforeEach(resetSession)
  afterEach(() => vi.unstubAllGlobals())

  it('成功时写入用户、角色、权限', async () => {
    mockMe({ user: { id: 'u1', username: 'admin', displayName: '管理员' }, roles: ['admin'], permissions: ['user:read'] })
    await loadSession()
    expect(session.user?.username).toBe('admin')
    expect(session.roles).toEqual(['admin'])
    expect(session.permissions).toEqual(['user:read'])
    expect(session.ready).toBe(true)
  })

  it('缺字段时兜底成空数组，不抛错', async () => {
    mockMe({ user: { id: 'u1', username: 'admin', displayName: '管理员' } })
    await loadSession()
    expect(session.roles).toEqual([])
    expect(session.permissions).toEqual([])
  })

  it('401 时清空会话但仍然标记 ready（否则路由守卫会卡住）', async () => {
    session.user = { id: 'old', username: 'old', displayName: '旧' }
    session.permissions = ['user:read']
    mockMe({ error: 'unauthorized', message: '未登录' }, 401)
    await loadSession()
    expect(session.user).toBeNull()
    expect(session.permissions).toEqual([])
    expect(session.ready).toBe(true)
  })
})

describe('login / logout', () => {
  beforeEach(resetSession)
  afterEach(() => vi.unstubAllGlobals())

  it('登录成功写入会话', async () => {
    mockMe({ user: { id: 'u1', username: 'admin', displayName: '管理员' }, roles: ['admin'], permissions: ['user:read'] })
    await login('admin', 'pw')
    expect(session.user?.username).toBe('admin')
    expect(session.ready).toBe(true)
  })

  it('登录失败抛出错误且不写入会话', async () => {
    mockMe({ error: 'bad_request', message: '用户名或密码错误' }, 400)
    await expect(login('admin', 'wrong')).rejects.toThrow('用户名或密码错误')
    expect(session.user).toBeNull()
  })

  it('登出即使接口失败也要清空本地会话', async () => {
    session.user = { id: 'u1', username: 'admin', displayName: '管理员' }
    session.permissions = ['user:read']
    vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 500 })))
    await expect(logout()).resolves.toBeUndefined()
    expect(session.user).toBeNull()
    expect(session.permissions).toEqual([])
  })
})
