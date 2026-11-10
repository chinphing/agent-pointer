import type { RouteComponent } from 'vue-router'

import Overview from './pages/Overview.vue'
import Members from './pages/Members.vue'
import Roles from './pages/Roles.vue'
import Resources from './pages/Resources.vue'
import Usage from './pages/Usage.vue'
import Audit from './pages/Audit.vue'
import Login from './pages/Login.vue'
import Home from './pages/Home.vue'
import Download from './pages/Download.vue'
import Me from './pages/me/Index.vue'
import NoAccess from './pages/NoAccess.vue'
import NotFound from './pages/NotFound.vue'

/**
 * 页面 → 板块 → 权限的唯一真源。
 *
 * 顶部导航的四个板块、板块内的侧边栏、登录后的落地页全部由这张表派生：
 * 新增页面只改这里，不会出现「菜单里有、路由里没有」或「页面能进、接口 403」的漂移。
 *
 * - `section` 省略 = 不属于任何板块（登录页、无权限页、404）
 * - `perm` 省略 = 登录即可；数组 = 任一命中即可见
 * - `public` = 无需登录
 */
export type Section = 'home' | 'download' | 'account' | 'console'

export interface AdminRoute {
  path: string
  /** i18n key，导航与页面标题共用。 */
  titleKey: string
  component: RouteComponent
  public?: boolean
  perm?: string | string[]
  /** 属于哪个板块。 */
  section?: Section
  /** 板块内顺序（侧边栏排序）。 */
  order?: number
  /** 顶栏全局搜索的形态。 */
  search?: 'global' | 'member' | 'resource' | 'billing'
}

/** 顶部导航的一个板块。 */
export interface SectionDef {
  key: Section
  labelKey: string
  /** 需要登录才出现。 */
  requiresLogin?: boolean
  /** 侧边栏只在 account / console 两个板块出现。 */
  hasSidebar?: boolean
}

/**
 * 四个板块，顺序即顶部导航顺序。
 *
 * 个人中心与控制面板各自有左侧栏；首页与下载是独立页面，只有顶部导航。
 */
export const SECTIONS: SectionDef[] = [
  { key: 'home', labelKey: 'section.home' },
  { key: 'download', labelKey: 'section.download' },
  { key: 'account', labelKey: 'section.account', requiresLogin: true, hasSidebar: true },
  { key: 'console', labelKey: 'section.console', requiresLogin: true, hasSidebar: true },
]

/** 资源页跨多种 kind，任一域有读权限就可见。 */
const RESOURCE_READ_PERMS = [
  'provider:read',
  'model:read',
  'skill:read',
  'plugin:read',
  'mcp:read',
]

export const ROUTES: AdminRoute[] = [
  {
    path: '/',
    // 首页内容待定，页头标题留空 —— 否则导航「首页」和内容区标题会重复两遍。
    titleKey: '',
    component: Home,
    public: true,
    section: 'home',
    order: 10,
  },
  {
    path: '/download',
    // 下载页自己渲染居中标题（对齐官网），外壳不再重复一遍。
    titleKey: '',
    component: Download,
    public: true,
    section: 'download',
    order: 10,
  },
  {
    path: '/login',
    titleKey: 'nav.login',
    component: Login,
    public: true,
  },
  { path: '/no-access', titleKey: 'nav.noAccess', component: NoAccess },
  {
    path: '/me',
    titleKey: 'nav.me',
    component: Me,
    section: 'account',
    order: 10,
  },
  {
    path: '/me/billing',
    titleKey: 'nav.meBilling',
    component: () => import('./pages/me/Billing.vue'),
    section: 'account',
    order: 20,
  },
  {
    path: '/me/usage',
    titleKey: 'nav.meUsage',
    component: () => import('./pages/me/Usage.vue'),
    section: 'account',
    order: 30,
  },
  {
    path: '/me/settings',
    titleKey: 'nav.meSettings',
    component: () => import('./pages/me/Settings.vue'),
    section: 'account',
    order: 40,
  },
  {
    path: '/admin/overview',
    titleKey: 'nav.overview',
    component: Overview,
    perm: 'system:read',
    section: 'console',
    order: 10,
  },
  {
    path: '/admin/members',
    titleKey: 'nav.members',
    component: Members,
    perm: 'user:read',
    section: 'console',
    order: 20,
  },
  {
    path: '/admin/roles',
    titleKey: 'nav.roles',
    component: Roles,
    perm: 'role:read',
    section: 'console',
    order: 30,
  },
  {
    path: '/admin/resources',
    titleKey: 'nav.resources',
    component: Resources,
    perm: RESOURCE_READ_PERMS,
    section: 'console',
    order: 40,
  },
  {
    path: '/admin/releases',
    titleKey: 'nav.releases',
    component: () => import('./pages/Releases.vue'),
    perm: 'release:read',
    section: 'console',
    order: 50,
  },
  {
    path: '/admin/usage',
    titleKey: 'nav.usage',
    component: Usage,
    perm: 'usage:read',
    section: 'console',
    order: 60,
  },
  {
    path: '/admin/billing',
    titleKey: 'nav.billing',
    component: () => import('./pages/Billing.vue'),
    perm: 'usage:read',
    section: 'console',
    order: 70,
  },
  {
    path: '/admin/audit',
    titleKey: 'nav.audit',
    component: Audit,
    perm: 'audit:read',
    section: 'console',
    order: 80,
  },
  // 兜底必须放最后：vue-router 按声明顺序匹配，放前面会吞掉后面所有路由。
  {
    path: '/:pathMatch(.*)*',
    titleKey: 'notFound.title',
    component: NotFound,
    public: true,
  },
]

/** 这条路由在当前权限下能不能看。省略 `perm` 表示登录即可。 */
export function canView(perm: string | string[] | undefined, granted: string[]): boolean {
  if (!perm) return true
  const list = Array.isArray(perm) ? perm : [perm]
  return list.some((p) => granted.includes(p))
}

/** 某个板块下当前用户能看的页面，按 order 排。 */
export function tabsFor(section: Section, granted: string[]): AdminRoute[] {
  return ROUTES.filter(
    (r) => r.section === section && canView(r.perm, granted)
  ).sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
}

/** 点击板块时去哪个页面：首页/下载是固定路径，其余取该板块第一页。 */
export function sectionEntry(section: Section, granted: string[]): string {
  if (section === 'home') return '/'
  if (section === 'download') return '/download'
  return tabsFor(section, granted)[0]?.path ?? '/no-access'
}

export interface TopNavItem {
  key: Section
  labelKey: string
  path: string
}

/**
 * 顶部导航的四个板块。
 *
 * 未登录时只剩首页与下载；控制面板要有至少一页可见权限才出现，
 * 零权限账号看不到它，而不是点进去被弹回来。
 */
export function topNavFor(granted: string[], loggedIn: boolean): TopNavItem[] {
  return SECTIONS.filter((s) => {
    if (s.requiresLogin && !loggedIn) return false
    if (s.key === 'console') return tabsFor('console', granted).length > 0
    return true
  }).map((s) => ({ key: s.key, labelKey: s.labelKey, path: sectionEntry(s.key, granted) }))
}

/**
 * 登录后落在哪：有管理端权限就进控制面板第一页，否则退到个人中心 `/me`。
 * 一个权限都没有的用户也会落在 `/me`（登录即可），而不是撞在管理员概览页上吃 403。
 */
export function landingFor(granted: string[]): string {
  const consoleTabs = tabsFor('console', granted)
  if (consoleTabs.length) return consoleTabs[0].path
  return '/me'
}
