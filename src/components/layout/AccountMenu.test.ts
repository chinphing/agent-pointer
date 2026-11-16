// @vitest-environment happy-dom
//
// The bottom-left account capsule must show who is signed in, in every
// deployment mode. Standalone installs used to short-circuit to "本地模式",
// which hid the identity of users signed in through local_password / OIDC /
// OAuth2 providers.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createApp, nextTick, type App } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'

vi.mock('../../composables/usePlatformBalance', () => ({
  usePlatformBalance: () => ({
    balance: { value: null },
    loading: { value: false },
    visible: { value: false },
    lowBalance: { value: false },
    exhausted: { value: false },
    refresh: () => {}
  })
}))

vi.mock('../../composables/useAppUpdater', () => ({
  useAppUpdater: () => ({
    updating: { value: false },
    downloading: { value: false },
    checking: { value: false },
    statusMessage: { value: '' },
    updateReady: { value: false },
    updateVersion: { value: '' },
    check: () => {},
    download: () => {},
    install: () => {}
  })
}))

vi.mock('../../composables/useAppAdmin', () => ({
  useIsAppAdmin: () => ({ value: false })
}))

vi.mock('../../lib/webBranding', () => ({
  resolveBrandIcon: () => ''
}))

import AccountMenu from './AccountMenu.vue'
import { usePlatformAuthStore } from '../../stores/platformAuth'

function i18n() {
  return createI18n({
    legacy: false,
    locale: 'zh-CN',
    messages: {
      'zh-CN': {
        shell: {
          localMode: '本地模式',
          notLoggedIn: '未登录',
          loggedIn: '已登录',
          balanceYuan: '余额 ¥{amount}',
          checkUpdate: '检查更新',
          checkingUpdate: '检查中…',
          updateReady: '更新已就绪',
          settings: '设置',
          theme: '主题',
          logout: '退出登录',
          login: '登录'
        }
      }
    }
  })
}

type State = {
  authMode: 'standalone' | 'platform'
  loggedIn: boolean
  nickname?: string | null
}

function mountMenu(state: State): { app: App; host: HTMLElement; title: () => string } {
  const pinia = createPinia()
  setActivePinia(pinia)
  const store = usePlatformAuthStore()
  store.authMode = state.authMode
  store.session = {
    logged_in: state.loggedIn,
    user_nickname: state.nickname ?? null
  }

  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp(AccountMenu)
  app.use(pinia)
  app.use(i18n())
  app.mount(host)
  return {
    app,
    host,
    title: () => host.querySelector('.account-menu-trigger span')?.textContent?.trim() ?? ''
  }
}

describe('account capsule title', () => {
  const mounted: App[] = []

  function mount(state: State) {
    const menu = mountMenu(state)
    mounted.push(menu.app)
    return menu
  }

  afterEach(() => {
    while (mounted.length > 0) {
      const app = mounted.pop()!
      app.unmount()
    }
    document.body.innerHTML = ''
  })

  it('shows the nickname of a signed-in standalone user, not "本地模式"', async () => {
    const menu = mount({ authMode: 'standalone', loggedIn: true, nickname: 'admin' })
    await nextTick()
    expect(menu.title()).toBe('admin')
  })

  it('shows the nickname of a signed-in SSO user on a standalone install', async () => {
    const menu = mount({ authMode: 'standalone', loggedIn: true, nickname: '张三' })
    await nextTick()
    expect(menu.title()).toBe('张三')
  })

  it('falls back to the generic signed-in label when the nickname is blank', async () => {
    const menu = mount({ authMode: 'standalone', loggedIn: true, nickname: '   ' })
    await nextTick()
    expect(menu.title()).toBe('已登录')
  })

  it('keeps "本地模式" for a signed-out standalone install', async () => {
    const menu = mount({ authMode: 'standalone', loggedIn: false })
    await nextTick()
    expect(menu.title()).toBe('本地模式')
  })

  it('shows "未登录" for a signed-out managed install', async () => {
    const menu = mount({ authMode: 'platform', loggedIn: false })
    await nextTick()
    expect(menu.title()).toBe('未登录')
  })

  it('shows the nickname for a signed-in managed install', async () => {
    const menu = mount({ authMode: 'platform', loggedIn: true, nickname: 'admin' })
    await nextTick()
    expect(menu.title()).toBe('admin')
  })
})
