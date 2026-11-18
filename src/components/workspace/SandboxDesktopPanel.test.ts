// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n, t } from '../../i18n'
import { applyUiLocale } from '../../lib/uiLocale'

/** 记录 noVNC 实例，方便断言"什么时候连、什么时候断、带了什么密码"。 */
const rfbInstances: MockRfb[] = []

class MockRfb {
  static lastOptions: Record<string, unknown> | null = null

  listeners = new Map<string, (event: Event) => void>()
  viewOnly = true
  focusOnClick = false
  scaleViewport = false
  clipViewport = true
  dragViewport = true
  focused = 0
  disconnected = 0

  constructor(
    public target: HTMLElement,
    public url: string,
    public options?: Record<string, unknown>
  ) {
    MockRfb.lastOptions = options ?? null
    rfbInstances.push(this)
  }

  addEventListener(type: string, listener: (event: Event) => void) {
    this.listeners.set(type, listener)
  }

  removeEventListener(type: string) {
    this.listeners.delete(type)
  }

  emit(type: string, event: Event = new Event(type)) {
    this.listeners.get(type)?.(event)
  }

  disconnect() {
    this.disconnected += 1
  }

  focus() {
    this.focused += 1
  }

  blur() {}
}

vi.mock('@novnc/novnc/lib/rfb.js', () => ({ default: MockRfb }))

const mountedApps: Array<ReturnType<typeof createApp>> = []

beforeEach(() => {
  applyUiLocale('zh-CN')
  rfbInstances.length = 0
  MockRfb.lastOptions = null
})

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

async function mountPanel(props: { wsUrl: string | null; password: string | null }) {
  const { default: SandboxDesktopPanel } = await import('./SandboxDesktopPanel.vue')
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(SandboxDesktopPanel, props)
  app.use(i18n)
  mountedApps.push(app)
  app.mount(host)
  await nextTick()
  return { host, app }
}

describe('SandboxDesktopPanel', () => {
  it('有 ws 地址时用密码连上，并聚焦画布', async () => {
    await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: 'abcd2345' })

    expect(rfbInstances).toHaveLength(1)
    expect(rfbInstances[0].url).toBe('ws://127.0.0.1:6123/websockify')
    expect(MockRfb.lastOptions).toEqual({ credentials: { password: 'abcd2345' } })
    expect(rfbInstances[0].viewOnly).toBe(false)

    rfbInstances[0].emit('connect')
    await nextTick()

    expect(rfbInstances[0].focused).toBe(1)
    expect(rfbInstances[0].scaleViewport).toBe(true)
  })

  it('没有 ws 地址时不建连接，并说明镜像不支持', async () => {
    const { host } = await mountPanel({ wsUrl: null, password: null })

    expect(rfbInstances).toHaveLength(0)
    expect(host.textContent).toContain(t('workspace.desktopUnsupported'))
  })

  it('卸载时断开连接（离开标签页就释放，不留常驻流）', async () => {
    await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    expect(rfbInstances).toHaveLength(1)

    for (const app of mountedApps.splice(0)) app.unmount()

    expect(rfbInstances[0].disconnected).toBe(1)
  })

  it('对端断开后提示可以重连', async () => {
    const { host } = await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    rfbInstances[0].emit('connect')
    await nextTick()
    rfbInstances[0].emit('disconnect')
    await nextTick()

    expect(host.textContent).toContain(t('workspace.desktopDisconnected'))
  })
})
