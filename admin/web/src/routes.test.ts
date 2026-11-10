import { describe, expect, it } from 'vitest'
import { canView, landingFor, ROUTES, SECTIONS, tabsFor, topNavFor } from './routes'

const ACCOUNT_PATHS = ['/me', '/me/billing', '/me/usage', '/me/settings']
const CONSOLE_PATHS = [
  '/admin/overview',
  '/admin/members',
  '/admin/roles',
  '/admin/resources',
  '/admin/releases',
  '/admin/usage',
  '/admin/billing',
  '/admin/audit',
]

const ALL_PERMS = [
  'system:read',
  'user:read',
  'role:read',
  'provider:read',
  'release:read',
  'usage:read',
  'audit:read',
]

describe('routes', () => {
  // 回归：catch-all 一旦被放到数组中间，vue-router 会按声明顺序把后面所有路由
  // 都匹配到 404 页（加这条路由时就差点这么写）。
  it('兜底路由必须是最后一条，且不参与任何板块', () => {
    const last = ROUTES[ROUTES.length - 1]
    expect(last.path).toBe('/:pathMatch(.*)*')
    expect(last.section).toBeUndefined()
    expect(tabsFor('account', ALL_PERMS).map((r) => r.path)).not.toContain(last.path)
    expect(tabsFor('console', ALL_PERMS).map((r) => r.path)).not.toContain(last.path)
  })

  it('真实路由都排在兜底之前', () => {
    const catchAll = ROUTES.findIndex((r) => r.path === '/:pathMatch(.*)*')
    expect(catchAll).toBe(ROUTES.length - 1)
    for (const p of [...ACCOUNT_PATHS, ...CONSOLE_PATHS, '/', '/download']) {
      const idx = ROUTES.findIndex((r) => r.path === p)
      expect(idx, `${p} 应排在兜底之前`).toBeGreaterThanOrEqual(0)
      expect(idx, `${p} 应排在兜底之前`).toBeLessThan(catchAll)
    }
  })

  it('四个板块，顺序固定：首页 / 下载 / 个人中心 / 控制面板', () => {
    expect(SECTIONS.map((s) => s.key)).toEqual(['home', 'download', 'account', 'console'])
  })

  it('未登录只看到首页与下载', () => {
    expect(topNavFor([], false).map((i) => i.key)).toEqual(['home', 'download'])
  })

  it('登录即出现个人中心；零权限不出现控制面板', () => {
    expect(topNavFor([], true).map((i) => i.key)).toEqual(['home', 'download', 'account'])
  })

  it('有任一管理端读权限才出现控制面板', () => {
    expect(topNavFor(['system:read'], true).map((i) => i.key)).toEqual([
      'home',
      'download',
      'account',
      'console',
    ])
  })

  it('点板块落到该板块第一页', () => {
    const nav = topNavFor(ALL_PERMS, true)
    expect(nav.find((i) => i.key === 'home')?.path).toBe('/')
    expect(nav.find((i) => i.key === 'download')?.path).toBe('/download')
    expect(nav.find((i) => i.key === 'account')?.path).toBe('/me')
    expect(nav.find((i) => i.key === 'console')?.path).toBe('/admin/overview')
  })

  it('个人中心侧边栏固定四项', () => {
    expect(tabsFor('account', []).map((r) => r.path)).toEqual(ACCOUNT_PATHS)
  })

  it('控制面板侧边栏按权限过滤、按 order 排', () => {
    expect(tabsFor('console', ALL_PERMS).map((r) => r.path)).toEqual(CONSOLE_PATHS)
    expect(tabsFor('console', ['audit:read']).map((r) => r.path)).toEqual(['/admin/audit'])
    expect(tabsFor('console', [])).toEqual([])
  })

  it('零权限用户落在 /me，而不是管理员概览', () => {
    expect(landingFor([])).toBe('/me')
  })

  it('有管理端权限就进控制面板第一页', () => {
    expect(landingFor(['system:read'])).toBe('/admin/overview')
    expect(landingFor(['audit:read'])).toBe('/admin/audit')
  })

  it('资源页按多种 kind 的读权限判定', () => {
    const has = (perms: string[]) =>
      tabsFor('console', perms).some((r) => r.path === '/admin/resources')
    expect(has(['skill:read'])).toBe(true)
    expect(has(['usage:read'])).toBe(false)
  })

  it('省略 perm 表示登录即可', () => {
    expect(canView(undefined, [])).toBe(true)
  })

  it('数组 perm 任一命中即可', () => {
    expect(canView(['provider:read', 'model:read'], ['model:read'])).toBe(true)
    expect(canView(['provider:read', 'model:read'], ['skill:read'])).toBe(false)
  })

  it('个人中心不挂在 /admin 下', () => {
    expect(ACCOUNT_PATHS.every((p) => !p.startsWith('/admin'))).toBe(true)
  })
})
