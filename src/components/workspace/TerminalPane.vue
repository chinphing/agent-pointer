<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { X } from 'lucide-vue-next'
import {
  applyImeFriendlyTextareaStyles,
  createTerminalImeGuard,
  isWebKitTerminalHost,
  shouldDeferKeyToIme,
  type TerminalImeGuard
} from '../../lib/terminalIme'
import { ensureTerminalFontsReady } from '../../lib/terminalFonts'
import { useConsoleStore } from '../../stores/console'

const props = defineProps<{
  workspaceRoot: string
  conversationId: string
  /** 绑定的 console 会话 id；null = 空窗格（不挂 xterm）。 */
  tabId: string | null
  focused: boolean
}>()

const emit = defineEmits<{ focus: []; close: [] }>()

const consoleStore = useConsoleStore()
const host = ref<HTMLElement | null>(null)

const tab = computed(() => {
  if (!props.tabId) return null
  return consoleStore.tabs.find(item => item.id === props.tabId) ?? null
})

let terminal: import('@xterm/xterm').Terminal | null = null
let fitAddon: import('@xterm/addon-fit').FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null
let opening: Promise<void> | null = null
let renderedSessionId = ''
let renderedOutputLength = 0
let removeSelectionGuard: (() => void) | null = null
let imeGuard: TerminalImeGuard | null = null
/** True between primary-button mousedown and mouseup on the terminal. */
let selectionPressing = false
/** xterm 6 manages scrollbar via its own `_scrollbarState`; on WKWebView its
 *  wheel handling can fail to move the viewport. Fallback scrolls the real
 *  `.xterm-viewport` when xterm itself did not move it. */
let viewportScrollFallback: ((e: WheelEvent) => void) | null = null
let fallbackViewport: HTMLElement | null = null

function showError(cause: unknown) {
  console.error('[TerminalPane]', cause)
}

function writeBoundSession(data: string) {
  if (!props.tabId || !data) return
  void consoleStore.write(props.tabId, data).catch(showError)
}

async function settleTerminalLayout() {
  // Panes mount beneath a view switch / split; the first ResizeObserver
  // callback can see an intermediate width, which used to let the PTY print
  // its initial prompt before xterm had its final column count. Wait for
  // layout to settle before spawning/fitting.
  await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
  fitAddon?.fit()
}

function terminalSize() {
  return { cols: terminal?.cols || 80, rows: terminal?.rows || 24 }
}

function terminalTheme() {
  const styles = getComputedStyle(document.documentElement)
  const accent = `hsl(${styles.getPropertyValue('--accent').trim()})`
  const accentMuted = `hsl(${styles.getPropertyValue('--accent-muted').trim()})`
  return {
    background: `hsl(${styles.getPropertyValue('--card').trim()})`,
    foreground: `hsl(${styles.getPropertyValue('--foreground').trim()})`,
    cursor: accent,
    selectionBackground: accentMuted
  }
}

function syncTerminalTheme() {
  if (terminal) terminal.options.theme = terminalTheme()
}

async function ensureTerminal() {
  if (terminal || !host.value) return
  const [{ Terminal }, { FitAddon }, fontFamily] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-fit'),
    import('@xterm/xterm/css/xterm.css').then(() => ensureTerminalFontsReady())
  ])
  terminal = new Terminal({
    // Keep the pre-fit screen within the pane's minimum width. FitAddon
    // replaces this conservative value once the layout settles.
    cols: 30,
    cursorBlink: true,
    convertEol: false,
    // OS system monospace + default Chinese UI font (see terminalFonts.ts).
    fontFamily,
    fontSize: 12,
    scrollback: 5_000,
    // On macOS, Option+drag otherwise enters column-select (tall rectangle over
    // empty cells). Force Option to mean "selection", not column mode.
    macOptionClickForcesSelection: true,
    theme: terminalTheme()
  })
  const webkitIme = isWebKitTerminalHost()
  if (webkitIme) {
    imeGuard = createTerminalImeGuard(writeBoundSession)
  }
  // Let the browser/IME own composition keys (Safari may use 0 / Process / Dead
  // instead of keyCode 229 on the first stroke).
  terminal.attachCustomKeyEventHandler(e => {
    imeGuard?.observeKeyEvent(e)
    return !shouldDeferKeyToIme(e)
  })
  fitAddon = new FitAddon()
  terminal.loadAddon(fitAddon)
  terminal.open(host.value)
  installSelectionGuard(terminal.element)
  const ta = terminal.textarea
  if (ta) {
    applyImeFriendlyTextareaStyles(ta)
    imeGuard?.attach(ta)
  }
  terminal.onData(data => {
    writeBoundSession(imeGuard ? imeGuard.filterData(data) : data)
  })
  // Clicking the host (or empty cells) must focus xterm so vim keys reach the PTY.
  terminal.element?.addEventListener('mousedown', () => {
    emit('focus')
    terminal?.focus()
  })
  themeObserver = new MutationObserver(syncTerminalTheme)
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] })
  resizeObserver = new ResizeObserver(() => { void syncSize() })
  resizeObserver.observe(host.value)
  await nextTick()
  await settleTerminalLayout()
  fitAddon.fit()
  installViewportScrollFallback()
}

/**
 * Keep normal click-drag selection. Only guard the stuck-drag case: if mouseup
 * is lost (common when releasing outside the webview), xterm still has its
 * document mousemove hook and will keep painting a selection while the button
 * is already up — that looks like "click, then move anywhere selects".
 */
function installSelectionGuard(element: HTMLElement | undefined | null) {
  removeSelectionGuard?.()
  removeSelectionGuard = null
  selectionPressing = false
  if (!element) return

  const onMouseDownCapture = (event: MouseEvent) => {
    if (event.button !== 0) return
    selectionPressing = true
  }

  const onMouseUpCapture = (event: MouseEvent) => {
    if (event.button !== 0) return
    selectionPressing = false
  }

  const onMouseMoveCapture = (event: MouseEvent) => {
    if (!selectionPressing) return
    if (event.buttons & 1) return
    // Button is up but we never saw mouseup — stop extending and finalize.
    selectionPressing = false
    event.stopImmediatePropagation()
    document.dispatchEvent(
      new MouseEvent('mouseup', {
        bubbles: true,
        cancelable: true,
        view: window,
        button: 0,
        buttons: 0,
        clientX: event.clientX,
        clientY: event.clientY
      })
    )
  }

  element.addEventListener('mousedown', onMouseDownCapture, true)
  document.addEventListener('mouseup', onMouseUpCapture, true)
  document.addEventListener('mousemove', onMouseMoveCapture, true)

  removeSelectionGuard = () => {
    element.removeEventListener('mousedown', onMouseDownCapture, true)
    document.removeEventListener('mouseup', onMouseUpCapture, true)
    document.removeEventListener('mousemove', onMouseMoveCapture, true)
    selectionPressing = false
  }
}

/**
 * WKWebView fallback for xterm 6 wheel scrolling. xterm's Viewport consumes
 * the wheel event and moves the scrollbar via its own state machine; when that
 * silently fails (WebKit), the real `.xterm-viewport` never moves and the
 * overlay scrollbar never appears. If xterm did not change scrollTop, apply
 * the delta ourselves so users can always scroll long output.
 */
function installViewportScrollFallback() {
  removeViewportScrollFallback()
  if (!host.value || !terminal) return
  fallbackViewport = host.value.querySelector<HTMLElement>('.xterm-viewport')
  if (!fallbackViewport) return
  viewportScrollFallback = (event: WheelEvent) => {
    const vp = fallbackViewport
    if (!vp || vp.scrollHeight <= vp.clientHeight || event.deltaY === 0) return
    const before = vp.scrollTop
    requestAnimationFrame(() => {
      // xterm scrolled synchronously on this event → no fallback needed.
      if (vp.scrollTop === before) {
        vp.scrollTop += event.deltaY
        event.preventDefault()
      }
    })
  }
  host.value.addEventListener('wheel', viewportScrollFallback, { capture: true })
}

function removeViewportScrollFallback() {
  if (host.value && viewportScrollFallback) {
    host.value.removeEventListener('wheel', viewportScrollFallback, { capture: true } as EventListenerOptions)
  }
  viewportScrollFallback = null
  fallbackViewport = null
}

async function syncSize() {
  const id = props.tabId
  if (!terminal || !fitAddon || !id) return
  fitAddon.fit()
  const { cols, rows } = terminalSize()
  try {
    await consoleStore.resize(id, cols, rows)
  } catch (cause) {
    showError(cause)
  }
}

/** 增量渲染绑定会话的 output（xterm 6 队列语义与单实例版一致）。 */
async function renderTab() {
  if (!terminal) return
  const current = tab.value
  if (!current) {
    // xterm 6.0.0 queues writes (parsed via setTimeout); clear() is
    // synchronous and does not drain the queue, so pending bytes from
    // the previous tab can be parsed into the view after the wipe.
    // Write the ANSI erase through the queue so it is ordered after
    // any pending writes from the old tab.
    terminal.write('\x1b[2J\x1b[3J\x1b[H')
    renderedSessionId = ''
    renderedOutputLength = 0
    return
  }
  if (renderedSessionId !== current.id || renderedOutputLength > current.output.length) {
    terminal.write('\x1b[2J\x1b[3J\x1b[H')
    renderedSessionId = current.id
    renderedOutputLength = 0
  }
  const pending = current.output.slice(renderedOutputLength)
  if (!pending) return
  terminal.write(pending)
  renderedOutputLength = current.output.length
}

async function boot() {
  if (opening) return opening
  opening = (async () => {
    await ensureTerminal()
    await renderTab()
    await syncSize()
  })()
  return opening
}

function focusPane() {
  emit('focus')
  requestAnimationFrame(() => terminal?.focus())
}

watch(() => props.tabId, async (next) => {
  if (next) {
    await boot()
  } else {
    // 解绑：清屏但不销毁 xterm（窗格可能马上绑定另一个会话）。
    if (terminal) {
      terminal.write('\x1b[2J\x1b[3J\x1b[H')
    }
    renderedSessionId = ''
    renderedOutputLength = 0
  }
})
watch(() => tab.value?.output, () => { void renderTab() })
watch(() => props.focused, (next) => {
  if (next) requestAnimationFrame(() => terminal?.focus())
})

onMounted(() => {
  if (props.tabId) void boot()
})
onBeforeUnmount(() => {
  removeSelectionGuard?.()
  removeSelectionGuard = null
  removeViewportScrollFallback()
  imeGuard?.detach()
  imeGuard = null
  resizeObserver?.disconnect()
  resizeObserver = null
  themeObserver?.disconnect()
  themeObserver = null
  terminal?.dispose()
  terminal = null
  fitAddon = null
  opening = null
})
</script>

<template>
  <div
    class="terminal-pane flex h-full min-h-0 min-w-0 flex-col"
    :class="{ 'is-focused': focused }"
    @mousedown.self="focusPane"
  >
    <div v-show="tab" ref="host" class="terminal-host min-h-0 flex-1" data-terminal-pane-host />
    <div v-if="!tab" class="pane-empty flex min-h-0 flex-1 items-center justify-center px-3 text-center text-xs text-slate-400">
      <p>此窗格暂无终端</p>
    </div>
    <button
      v-if="tab"
      type="button"
      class="pane-close"
      :title="focused ? '关闭此窗格' : '关闭此窗格'"
      aria-label="关闭此窗格"
      @click.stop="emit('close')"
    ><X class="h-3 w-3" /></button>
  </div>
</template>

<style scoped>
.terminal-pane { position: relative; background: hsl(var(--card)); }
.terminal-pane.is-focused { outline: 1px solid hsl(var(--accent)); outline-offset: -1px; }
.terminal-pane :deep(.xterm) { height: 100%; }
.terminal-pane :deep(.xterm-screen) { user-select: none; }
.terminal-host { scrollbar-gutter: stable; user-select: none; }
/*
 * WKWebView / Safari: xterm's helper textarea defaults to opacity:0 and
 * z-index:-5 (css + runtime inline). WebKit will not open a system IME for
 * that "invisible" field, so Chinese composition never starts (Chromium is
 * fine). Keep the field engine-visible but visually transparent; xterm still
 * positions/sizes it under the cursor.
 */
.terminal-pane :deep(.xterm-helper-textarea) {
  opacity: 1 !important;
  z-index: 1 !important;
  color: transparent !important;
  background: transparent !important;
  caret-color: transparent !important;
  text-shadow: none !important;
  -webkit-text-fill-color: transparent !important;
  -webkit-user-select: text !important;
  user-select: text !important;
}
.terminal-pane :deep(.xterm-viewport) { overflow-y: auto !important; overscroll-behavior: contain; }
.pane-close {
  @apply absolute right-1 top-1 z-10 rounded p-1 text-muted opacity-0 transition-opacity;
}
.terminal-pane:hover .pane-close { @apply opacity-70; }
.pane-close:hover { @apply bg-hover text-foreground opacity-100; }
</style>
