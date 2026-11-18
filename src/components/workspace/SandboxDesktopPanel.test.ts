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
  blurred = 0
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

  blur() {
    this.blurred += 1
  }
}

vi.mock('@novnc/novnc/lib/rfb.js', () => ({ default: MockRfb }))

const pinCalls: Array<[string, boolean]> = []
vi.mock('../../lib/api', () => ({
  setSandboxDesktopActive: (id: string, active: boolean) => {
    pinCalls.push([id, active])
    return Promise.resolve()
  },
}))

const mountedApps: Array<ReturnType<typeof createApp>> = []

beforeEach(() => {
  applyUiLocale('zh-CN')
  rfbInstances.length = 0
  pinCalls.length = 0
  MockRfb.lastOptions = null
})

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

async function mountPanel(props: {
  wsUrl: string | null
  password: string | null
  conversationId?: string
}) {
  const { default: SandboxDesktopPanel } = await import('./SandboxDesktopPanel.vue')
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp(SandboxDesktopPanel, { conversationId: 'conv-1', ...props })
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
    // 默认只读：容器桌面常常正被 agent 操作，不能一上来就抢键鼠。
    expect(rfbInstances[0].viewOnly).toBe(true)

    rfbInstances[0].emit('connect')
    await nextTick()

    // 只读态不抢焦点
    expect(rfbInstances[0].focused).toBe(0)
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

  it('接管页打开时 pin、关闭时 unpin（避免接管期间被空闲回收）', async () => {
    await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    expect(pinCalls).toEqual([['conv-1', true]])

    for (const app of mountedApps.splice(0)) app.unmount()
    await nextTick()

    expect(pinCalls).toEqual([
      ['conv-1', true],
      ['conv-1', false],
    ])
  })

  it('1:1 用容器原生滚动，不依赖拖动平移', async () => {
    const { host } = await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    const rfb = rfbInstances[0]
    rfb.emit('connect')
    await nextTick()

    // 滚动容器必须是 .desktop-screen 的**外层**：noVNC 会把 .desktop-screen
    // 自己撑成 1920×1080，overflow 加在它身上就没有可滚动的溢出了。
    const screen = host.querySelector('.desktop-screen') as HTMLElement
    const scroller = screen.parentElement as HTMLElement
    expect(screen.className).not.toContain('overflow')
    expect(scroller.className).toContain('overflow-hidden')
    expect(rfb.scaleViewport).toBe(true)

    // 真实 canvas：noVNC 只把 canvas 设成帧缓冲尺寸，承载盒要自己跟上，
    // 否则外层没有可滚动溢出（实测 scrollWidth 一直等于 clientWidth）。
    const canvas = document.createElement('canvas')
    canvas.width = 1920
    canvas.height = 1080
    screen.appendChild(canvas)

    const btn = [...host.querySelectorAll('button')].find((b) =>
      b.getAttribute('title')?.includes(t('workspace.desktopScaleActual'))
    )
    btn!.click()
    await nextTick()

    expect(screen.style.width).toBe('1920px')
    expect(screen.style.height).toBe('1080px')

    // 回归：只读态下 noVNC 的拖动平移不生效，1:1 必须能靠滚动看右边和下面。
    expect(rfb.scaleViewport).toBe(false)
    expect(rfb.dragViewport).toBe(false)
    expect(scroller.className).toContain('overflow-auto')
  })

  it('全屏只铺满顶栏以下，不盖住窗口标题栏', async () => {
    const { host } = await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    const root = host.firstElementChild as HTMLElement
    const btn = host.querySelector<HTMLElement>(`[title="${t('workspace.desktopFullscreen')}"]`)
    expect(btn).toBeTruthy()

    btn!.click()
    await nextTick()

    // 回归：全屏必须给窗口控件让出顶部 40px，否则「已接管」那行会被红绿灯/标题压住。
    // macOS 用 pt-10（红绿灯是原生浮层），其它平台用 top-10（保住自绘窗口按钮）。
    expect(root.className).toContain('fixed')
    expect(root.className).toMatch(/\b(pt-10|top-10)\b/)

    // 全屏底衬是纯黑，主题灰在上面看不见——工具栏必须切到高对比配色。
    const toolbar = root.querySelector('.desktop-toolbar') as HTMLElement
    expect(toolbar.className).toContain('is-fullscreen')
    // 未连接时只有全屏按钮常驻，逐个检查更稳。
    const btns = [...toolbar.querySelectorAll('button')]
    expect(btns.length).toBeGreaterThan(0)
    for (const b of btns) expect(b.className).toContain('desktop-toolbar-btn')
  })

  it('点半透明按钮才接管键鼠，Esc/释放退回只读', async () => {
    const { host } = await mountPanel({ wsUrl: 'ws://127.0.0.1:6123/websockify', password: null })
    const rfb = rfbInstances[0]
    rfb.emit('connect')
    await nextTick()

    expect(rfb.viewOnly).toBe(true)
    const overlay = [...host.querySelectorAll('button')].find((b) =>
      b.textContent?.includes(t('workspace.desktopTakeControl'))
    )
    expect(overlay).toBeTruthy()

    overlay!.click()
    await nextTick()
    expect(rfb.viewOnly).toBe(false)
    expect(rfb.focused).toBe(1)
    // 接管后遮罩消失，画布直接收键鼠
    expect(
      [...host.querySelectorAll('button')].some((b) =>
        b.textContent?.includes(t('workspace.desktopTakeControl'))
      )
    ).toBe(false)

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await nextTick()
    expect(rfb.viewOnly).toBe(true)
    expect(rfb.blurred).toBe(1)
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
