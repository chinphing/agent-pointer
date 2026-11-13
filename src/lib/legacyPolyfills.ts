/**
 * Chrome 63 缺失的浏览器 API。
 *
 * 语法层由 esbuild 按 `chrome63` 降级（见 `vite.config.ts` 的 `modernTargets`），
 * core-js 的补丁由 `@vitejs/plugin-legacy` 的 `modernPolyfills` 注入——但下面这几个
 * **不在 core-js 覆盖范围内**，必须自己补，否则旧浏览器上是运行时报错：
 *
 * | API                | Chrome 起始 | 缺失时的表现                         |
 * | ------------------ | ----------- | ------------------------------------ |
 * | `ResizeObserver`   | 64          | 布局观察全部失效，弹窗/侧栏尺寸不更新 |
 * | `AbortController`  | 66          | 首屏直接抛 `AbortController is not defined`，页面只剩壳 |
 * | `crypto.randomUUID`| 92          | 新建会话报错                         |
 * | `queueMicrotask`   | 71          | 个别调度逻辑抛错                     |
 *
 * 每一项都是「缺失才安装」，现代浏览器零开销、零行为变化。
 */

import ResizeObserverPolyfill from 'resize-observer-polyfill'
// UMD 自带 `polyfillNeeded()` 判断：只在 AbortController 缺失（或 fetch 不支持
// signal）时才挂全局，现代浏览器进来直接 return。只补构造函数，不动 fetch。
import 'abortcontroller-polyfill/dist/abortcontroller-polyfill-only'

/** 最小 randomUUID 实现：v4，够用即可，不追求密码学强度以外的属性。 */
function randomUuidV4(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  // RFC 4122 §4.4：版本号 4，变体位 10xx
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex: string[] = []
  for (let i = 0; i < 16; i += 1) {
    hex.push((bytes[i] + 0x100).toString(16).slice(1))
  }
  return [
    hex.slice(0, 4).join(''),
    hex.slice(4, 6).join(''),
    hex.slice(6, 8).join(''),
    hex.slice(8, 10).join(''),
    hex.slice(10, 16).join('')
  ].join('-')
}

/**
 * Flex `gap` landed in Chrome 84 — `@supports (gap: 1px)` cannot tell it apart
 * from grid `gap` (Chrome 66 already has that), so feature detection has to
 * measure a real flex layout. When it is missing we tag `<html>` and the
 * stylesheet supplies margin fallbacks; modern browsers never get the class.
 */
function markFlexGapSupport(): void {
  if (typeof document === 'undefined' || !document.body) return
  const probe = document.createElement('div')
  probe.setAttribute('aria-hidden', 'true')
  probe.style.cssText =
    'position:absolute;top:-9999px;left:-9999px;display:flex;flex-direction:column;gap:1px;visibility:hidden'
  const first = document.createElement('div')
  const second = document.createElement('div')
  first.style.cssText = 'height:1px;width:1px'
  second.style.cssText = 'height:1px;width:1px'
  probe.append(first, second)
  document.body.appendChild(probe)
  const supported =
    second.getBoundingClientRect().top - first.getBoundingClientRect().bottom >= 1
  probe.remove()
  document.documentElement.classList.toggle('no-flex-gap', !supported)
}

export function installLegacyPolyfills(): void {
  if (typeof window === 'undefined') return

  if (!('ResizeObserver' in window)) {
    ;(window as unknown as { ResizeObserver: unknown }).ResizeObserver =
      ResizeObserverPolyfill
  }

  markFlexGapSupport()

  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID !== 'function') {
    ;(crypto as unknown as { randomUUID: () => string }).randomUUID = randomUuidV4
  }

  if (typeof window.queueMicrotask !== 'function') {
    window.queueMicrotask = (callback: () => void): void => {
      // 用 Promise 微任务兜底，语义与原生一致（在当前任务之后、渲染之前执行）。
      Promise.resolve()
        .then(callback)
        .catch((err) => {
          // 与原生一致：回调抛错走全局未处理异常，而不是静默吞掉。
          setTimeout(() => {
            throw err
          }, 0)
        })
    }
  }
}
