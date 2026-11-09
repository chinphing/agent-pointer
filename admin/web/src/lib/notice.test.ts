import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { effectScope } from 'vue'
import { useNotice } from './notice'

/**
 * 挡的是「保存成功」提示一直挂在页面上的问题：
 * 设完必须自己消失，换页签要能手动清，重复设置不能叠加定时器。
 */
describe('useNotice', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  function inScope() {
    const scope = effectScope()
    const api = scope.run(() => useNotice(3000))!
    return { ...api, stop: () => scope.stop() }
  }

  it('到点自动消失', () => {
    const n = inScope()
    n.flash('模型服务已保存')
    expect(n.notice.value).toBe('模型服务已保存')
    vi.advanceTimersByTime(2999)
    expect(n.notice.value).toBe('模型服务已保存')
    vi.advanceTimersByTime(1)
    expect(n.notice.value).toBe('')
    n.stop()
  })

  it('连续两次设置不会提前消失', () => {
    const n = inScope()
    n.flash('第一条')
    vi.advanceTimersByTime(2000)
    n.flash('第二条')
    vi.advanceTimersByTime(2000)
    // 第二条才过 2 秒，应该还在
    expect(n.notice.value).toBe('第二条')
    vi.advanceTimersByTime(1000)
    expect(n.notice.value).toBe('')
    n.stop()
  })

  it('clear 立即清掉并取消定时器', () => {
    const n = inScope()
    n.flash('已保存')
    n.clear()
    expect(n.notice.value).toBe('')
    vi.advanceTimersByTime(5000)
    expect(n.notice.value).toBe('')
    n.stop()
  })

  it('作用域销毁后不再改状态', () => {
    const n = inScope()
    n.flash('已保存')
    n.stop()
    expect(n.notice.value).toBe('')
  })
})
