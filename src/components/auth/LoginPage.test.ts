// @vitest-environment happy-dom
//
// Regression guard for the collapsed login panels and the "focused" password
// form.
//
// The form/panel is wrapped in `<Transition name="expand">`. When the leave hook
// forgot to call `done`, Vue kept the element in the DOM forever: height 0,
// `opacity: 0`, but the three `<input>`s stayed `tabIndex=0` — Tab and screen
// readers still reached them. Both halves are pinned here:
//   1. behaviour — after collapsing, no focusable form control is left behind;
//   2. contract  — the leave hook resolves `done` (a hook that ignores it is
//      exactly the bug, and would still pass a DOM-only test).
//
// The focused-form block pins the second rule: picking the secondary
// `local_password` entry hides every other entry and offers a way back.
import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import { createApp, nextTick, type App } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import LoginPage from './LoginPage.vue'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import type { AuthProvider } from '../../lib/api'

const PROVIDERS: AuthProvider[] = [
  { id: 'corp', display_name: '企业统一认证', kind: 'oidc', icon: 'shield', primary: true },
  { id: 'dingtalk', display_name: '钉钉', kind: 'oauth2_userinfo', icon: 'user' },
  { id: 'wecom', display_name: '企业微信', kind: 'oauth2_userinfo', icon: 'globe' },
  { id: 'local_password', display_name: '本地密码', kind: 'local_password', icon: 'key' },
]

function i18n() {
  return createI18n({
    legacy: false,
    locale: 'zh-CN',
    messages: {
      'zh-CN': {
        auth: {
          page: {
            title: '登录 Pointer',
            subtitle: '选择登录方式',
            or: '或',
            otherMethods: '其他登录方式（{n}）',
            button: '{name}登录',
            backToMethods: '返回其他登录方式',
            retryAfter: '{message}，请 {n} 秒后重试',
            cooldownSuffix: '（{n}s 后可重试）',
            usernameRequired: '请输入账号',
            passwordRequired: '请输入密码',
            captchaRequired: '请输入验证码',
            showPassword: '显示密码',
            hidePassword: '隐藏密码'
          },
          username: '账号',
          password: '密码',
          captcha: '验证码',
          refreshCaptcha: '点击刷新验证码',
          noProviders: '此实例未启用任何登录方式，请联系管理员',
          loginFailed: '登录失败，请重试'
        }
      }
    }
  })
}

function mountPage(options: { deferProviders?: boolean } = {}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const store = usePlatformAuthStore()
  store.providers = options.deferProviders ? [] : [...PROVIDERS]
  store.loading = false
  let releaseProviders: (() => void) | null = null
  if (options.deferProviders) {
    // Hold the registry load open so the skeleton window is observable.
    store.loadAuthProviders = () =>
      new Promise<void>(resolve => {
        releaseProviders = () => {
          store.providers = [...PROVIDERS]
          resolve()
        }
      })
  }
  // Never touch the network: the challenge is stubbed to a fixed SVG.
  store.requestLocalPasswordChallenge = async () => ({
    captchaId: 'test-captcha',
    imageSvg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>'
  })

  const host = document.createElement('div')
  document.body.appendChild(host)
  const app: App = createApp(LoginPage)
  app.use(pinia)
  app.use(i18n())
  app.mount(host)
  return {
    app,
    host,
    store,
    releaseProviders: () => releaseProviders?.()
  }
}

/** Collapse regions animate for 180ms; give the leave hook room to finish. */
async function settle() {
  await nextTick()
  await new Promise(resolve => setTimeout(resolve, 400))
  await nextTick()
}

describe('login page skeleton', () => {
  it('shows three placeholder rows (and no buttons) while providers load', async () => {
    const page = mountPage({ deferProviders: true })
    try {
      await nextTick()
      const host = page.host
      expect(host.querySelectorAll('[data-testid=login-skeleton]').length).toBe(3)
      expect(host.querySelectorAll('button').length).toBe(0)
      expect(host.textContent).not.toContain('此实例未启用任何登录方式')

      page.releaseProviders()
      await settle()

      expect(host.querySelectorAll('[data-testid=login-skeleton]').length).toBe(0)
      expect(host.querySelectorAll('button').length).toBeGreaterThan(0)
    } finally {
      page.app.unmount()
      page.host.remove()
    }
  })
})

describe('login page collapsed panels', () => {
  let mounted: ReturnType<typeof mountPage> | null = null

  beforeEach(() => {
    mounted = mountPage()
  })

  afterEach(() => {
    mounted?.app.unmount()
    mounted?.host.remove()
    mounted = null
  })

  it('removes the local password form (and its inputs) after Esc', async () => {
    const host = mounted!.host
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')
    expect(trigger).not.toBeNull()

    trigger!.click()
    await settle()
    expect(host.querySelectorAll('form input').length).toBe(3)

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await settle()

    expect(host.querySelectorAll('form').length).toBe(0)
    expect(host.querySelectorAll('form input').length).toBe(0)
    // No leftover focusable control from the collapsed form.
    expect(host.querySelectorAll('form input, form button').length).toBe(0)
    expect(document.querySelectorAll('form input').length).toBe(0)
  })

  it('keeps the form usable after collapsing and reopening', async () => {
    const host = mounted!.host
    const trigger = () =>
      host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')

    trigger()!.click()
    await settle()
    expect(host.querySelectorAll('form').length).toBe(1)

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await settle()
    expect(host.querySelectorAll('form').length).toBe(0)

    trigger()!.click()
    await settle()
    // Exactly one form: a stranded copy would show up as two.
    expect(host.querySelectorAll('form').length).toBe(1)
    expect(host.querySelectorAll('form input').length).toBe(3)
  })

  it('removes the "other methods" panel after Esc', async () => {
    const host = mounted!.host
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid=other-methods-trigger]')
    expect(trigger).not.toBeNull()

    trigger!.click()
    await settle()
    expect(host.querySelectorAll('#login-other-methods button').length).toBe(2)

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await settle()

    expect(host.querySelector('#login-other-methods')).toBeNull()
    expect(host.querySelectorAll('#login-other-methods button').length).toBe(0)
  })
})

describe('login page focused password form', () => {
  let mounted: ReturnType<typeof mountPage> | null = null

  beforeEach(() => {
    mounted = mountPage()
  })

  afterEach(() => {
    mounted?.app.unmount()
    mounted?.host.remove()
    mounted = null
  })

  it('hides every other entry and offers a way back', async () => {
    const host = mounted!.host
    host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')!.click()
    await settle()

    // Only the form and the back entry are left on screen.
    expect(host.querySelectorAll('form input').length).toBe(3)
    expect(host.querySelector('[data-testid=back-to-methods]')).not.toBeNull()
    expect(host.querySelector('[data-testid=other-methods-trigger]')).toBeNull()
    // The primary entry is gone, not merely invisible.
    expect(host.querySelectorAll('button').length).toBeGreaterThan(0)
    const labels = [...host.querySelectorAll('button')].map(button => button.textContent ?? '')
    expect(labels.some(text => text.includes('企业统一认证登录'))).toBe(false)
    expect(labels.some(text => text.includes('钉钉登录'))).toBe(false)
  })

  it('returns to the entry list from the back entry', async () => {
    const host = mounted!.host
    host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')!.click()
    await settle()

    host.querySelector<HTMLButtonElement>('[data-testid=back-to-methods]')!.click()
    await settle()

    expect(host.querySelectorAll('form').length).toBe(0)
    expect(host.querySelector('[data-testid=back-to-methods]')).toBeNull()
    expect(host.querySelector('[data-testid=other-methods-trigger]')).not.toBeNull()
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')
    expect(trigger).not.toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('returns to the entry list on Esc as well', async () => {
    const host = mounted!.host
    host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')!.click()
    await settle()

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await settle()

    expect(host.querySelectorAll('form').length).toBe(0)
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid=local-password-trigger]')
    expect(document.activeElement).toBe(trigger)
  })
})

describe('login page transition contract', () => {
  // happy-dom gives `import.meta.url` an http scheme, so resolve from cwd.
  const sfc = readFileSync(resolve(process.cwd(), 'src/components/auth/LoginPage.vue'), 'utf8')

  it('resolves `done` in the leave hook', () => {
    // The bug: `onLeave(el)` set height/opacity but never called `done`, so Vue
    // never removed the element. The hook must take the callback and use it.
    expect(sfc).toMatch(/function onLeave\(el: Element, done: \(\) => void\)/)
    expect(sfc).toMatch(/resolveWhenHeightSettles\(node, done\)/)
    expect(sfc).toMatch(/@leave="onLeave"/)
  })

  it('resolves `done` in the enter hook too', () => {
    expect(sfc).toMatch(/function onEnter\(el: Element, done: \(\) => void\)/)
    expect(sfc).toMatch(/@enter="onEnter"/)
  })

  it('marks collapsing panels inert so nothing is tabbable mid-animation', () => {
    expect(sfc).toMatch(/:inert="!showLocalForm \|\| undefined"/)
    expect(sfc).toMatch(/:inert="!otherMethodsOpen \|\| undefined"/)
  })
})

describe('page canvas background', () => {
  it('paints html with the same theme token as body', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf8')
    expect(css).toMatch(/html \{\s*background-color: hsl\(var\(--background\)\)/)
    expect(css).toMatch(/body \{\s*background: hsl\(var\(--background\)\)/)
  })

  it('keeps badge colours scoped to the badge, never to html', () => {
    // `:global(html.dark) .att-file-badge--x` compiles to a bare `html.dark`
    // rule and painted the whole canvas with a badge colour.
    const sfc = readFileSync(
      resolve(process.cwd(), 'src/components/chat/AttachmentFileIcon.vue'),
      'utf8'
    )
    expect(sfc).not.toMatch(/:global\(html\.dark\)\s+\.att-file-badge/)
  })
})
