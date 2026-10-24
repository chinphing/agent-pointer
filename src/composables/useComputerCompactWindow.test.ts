import { beforeEach, describe, expect, it, vi } from 'vitest'

const placeComputerCompactWindow =
  vi.fn<(width: number, height: number, margin: number) => Promise<void>>(async () => {})
const beginComputerCompactWindow = vi.fn(async () => {})
const restoreComputerCompactWindowNative = vi.fn(async () => {})

vi.mock('../lib/runtime', () => ({ isTauriRuntime: () => true }))
vi.mock('../lib/desktopOs', () => ({ detectDesktopOs: () => 'macos' }))
vi.mock('../lib/api', () => ({
  beginComputerCompactWindow: () => beginComputerCompactWindow(),
  placeComputerCompactWindow: (width: number, height: number, margin: number) =>
    placeComputerCompactWindow(width, height, margin),
  restoreComputerCompactWindowNative: () => restoreComputerCompactWindowNative()
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setBackgroundColor: async () => {} })
}))

/**
 * 回归：窗口被外部还原后再次进入收缩态，必须重新 resize。
 *
 * 旧实现里 `shrinkComputerCompactWindow` 开头有一行
 *   if (!resizeOnly && compactActive) return true
 * `compactActive` 是模块级标志，窗口被用户拖拽放大时它不会知道。于是第二次
 * 进入收缩态直接短路返回 true，调用方把 isCompact 置 true——界面变成收缩态的
 * 样子，窗口却还是全屏，收缩态的两端对齐布局被拉伸到整个屏幕。
 */
describe('shrinkComputerCompactWindow', () => {
  beforeEach(async () => {
    vi.resetModules()
    placeComputerCompactWindow.mockClear()
    beginComputerCompactWindow.mockClear()
    restoreComputerCompactWindowNative.mockClear()
  })

  it('每次调用都会 resize，即使已经处于收缩态', async () => {
    const mod = await import('./useComputerCompactWindow')

    await mod.shrinkComputerCompactWindow(false)
    await mod.shrinkComputerCompactWindow(false)

    // 两次都要真正落位，不能因为 compactActive 已置位就跳过第二次
    expect(placeComputerCompactWindow).toHaveBeenCalledTimes(2)
    // begin 只需要一次：它做的是「记录原始窗口状态」，重复调会把已缩小的尺寸
    // 当成原始状态存下来，展开时就还原不回去了
    expect(beginComputerCompactWindow).toHaveBeenCalledTimes(1)
  })

  it('两行状态变化时走 resizeOnly，不重复 begin', async () => {
    const mod = await import('./useComputerCompactWindow')

    await mod.shrinkComputerCompactWindow(false)
    await mod.shrinkComputerCompactWindow(true, true)

    expect(beginComputerCompactWindow).toHaveBeenCalledTimes(1)
    expect(placeComputerCompactWindow).toHaveBeenCalledTimes(2)
    // 第二次按两行高度落位
    expect(placeComputerCompactWindow).toHaveBeenLastCalledWith(400, 62, 16)
  })

  it('resize 失败时回滚原生窗口，并让调用方拿到 false', async () => {
    placeComputerCompactWindow.mockRejectedValueOnce(new Error('boom'))
    const mod = await import('./useComputerCompactWindow')

    const ok = await mod.shrinkComputerCompactWindow(false)

    expect(ok).toBe(false)
    expect(restoreComputerCompactWindowNative).toHaveBeenCalledTimes(1)
  })
})
