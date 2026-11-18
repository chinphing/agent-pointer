/**
 * `@novnc/novnc` 不带类型声明（1.5.0 是 babel 编译的 CJS，没有 .d.ts）。
 * 这里只声明我们用到的表面，避免 `any` 扩散到面板组件里。
 */
declare module '@novnc/novnc/lib/rfb.js' {
  export interface RfbCredentials {
    username?: string
    password?: string
    target?: string
  }

  export interface RfbOptions {
    credentials?: RfbCredentials
    shared?: boolean
    repeaterID?: string
    wsProtocols?: string[]
  }

  export default class RFB {
    constructor(target: HTMLElement, url: string, options?: RfbOptions)

    viewOnly: boolean
    focusOnClick: boolean
    clipViewport: boolean
    dragViewport: boolean
    scaleViewport: boolean
    resizeSession: boolean
    showDotCursor: boolean
    background: string

    disconnect(): void
    approveServer(): void
    sendCredentials(credentials: RfbCredentials): void
    sendKey(keysym: number, code: string | null, down?: boolean): void
    focus(options?: FocusOptions): void
    blur(): void
    clipboardPasteFrom(text: string): void

    addEventListener(type: string, listener: (event: Event) => void): void
    removeEventListener(type: string, listener: (event: Event) => void): void
  }
}
