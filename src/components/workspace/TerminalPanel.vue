<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Copy, FolderPlus, Loader2, RotateCcw, SquareTerminal, X } from 'lucide-vue-next'
import {
  applyImeFriendlyTextareaStyles,
  createTerminalImeGuard,
  isWebKitTerminalHost,
  shouldDeferKeyToIme,
  type TerminalImeGuard
} from '../../lib/terminalIme'
import { ensureTerminalFontsReady } from '../../lib/terminalFonts'
import { useConsoleStore, type WorkspaceConsoleTab } from '../../stores/console'

const props = defineProps<{ workspaceRoot: string; conversationId: string; active: boolean }>()

const consoleStore = useConsoleStore()
const host = ref<HTMLElement | null>(null)
const loading = ref(false)
const error = ref('')
const contextMenu = ref<{ tab: WorkspaceConsoleTab; x: number; y: number } | null>(null)
const workspaceTabs = computed(() =>
  consoleStore.tabs.filter(tab =>
    tab.workspaceRoot === props.workspaceRoot &&
    tab.conversationId === props.conversationId
  )
)
// Conversation-scoped active tab: the global store keeps one activeSessionId,
// so after switching conversations the stored id can still point at a tab from
// another conversation. Treat it as no tab for this panel.
const activeTab = computed(() =>
  workspaceTabs.value.find(tab => tab.id === consoleStore.activeSessionId) ?? null
)
const hasActiveTab = computed(() => !!activeTab.value)
const hasWorkspace = computed(() => !!props.workspaceRoot.trim())
let terminal: import('@xterm/xterm').Terminal | null = null
let fitAddon: import('@xterm/addon-fit').FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null
let opening: Promise<void> | null = null
let renderedSessionId = ''
let renderedOutputLength = 0
let removeOutsideMenuListeners: (() => void) | null = null
let removeSelectionGuard: (() => void) | null = null
let imeGuard: TerminalImeGuard | null = null
/** True between primary-button mousedown and mouseup on the terminal. */
let selectionPressing = false
/** xterm 6 manages scrollbar via its own `_scrollbarState`; on WKWebView its
 *  wheel handling can fail to move the viewport. Fallback scrolls the real
 *  `.xterm-viewport` when xterm itself did not move it. */
let viewportScrollFallback: ((e: WheelEvent) => void) | null = null
let fallbackViewport: HTMLElement | null = null

function writeActiveSession(data: string) {
  const id = consoleStore.activeSessionId
  if (!id || !data) return
  void consoleStore.write(id, data).catch(showError)
}

async function settleTerminalLayout() {
  // The TerminalPanel is async-mounted beneath a view switch. The first
  // ResizeObserver callback can see an intermediate width, which used to let
  // the PTY print its initial prompt before xterm had its final column count.
  // Wait for layout to settle before spawning a Shell.
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
    // Keep the pre-fit screen within the panel's minimum width. FitAddon
    // replaces this conservative value once the async-mounted view settles.
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
    imeGuard = createTerminalImeGuard(writeActiveSession)
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
    writeActiveSession(imeGuard ? imeGuard.filterData(data) : data)
  })
  // Clicking the host (or empty cells) must focus xterm so vim keys reach the PTY.
  terminal.element?.addEventListener('mousedown', () => {
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

function closeContextMenu() {
  contextMenu.value = null
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

function openContextMenu(event: MouseEvent, tab: WorkspaceConsoleTab) {
  contextMenu.value = { tab, x: event.clientX, y: event.clientY }
}

function installOutsideMenuListeners() {
  removeOutsideMenuListeners?.()
  const closeWhenOutside = (event: Event) => {
    const target = event.target as HTMLElement | null
    if (!target?.closest('.console-context-menu, .console-tab')) closeContextMenu()
  }
  const closeOnEscape = (event: KeyboardEvent) => {
    if (event.key === 'Escape') closeContextMenu()
  }
  window.addEventListener('pointerdown', closeWhenOutside, true)
  window.addEventListener('keydown', closeOnEscape)
  removeOutsideMenuListeners = () => {
    window.removeEventListener('pointerdown', closeWhenOutside, true)
    window.removeEventListener('keydown', closeOnEscape)
  }
}

function showError(cause: unknown) {
  error.value = cause instanceof Error ? cause.message : String(cause)
}

async function syncSize() {
  const id = consoleStore.activeSessionId
  if (!terminal || !fitAddon || !id) return
  fitAddon.fit()
  const { cols, rows } = terminalSize()
  try {
    await consoleStore.resize(id, cols, rows)
  } catch (cause) {
    showError(cause)
  }
}

async function createTab(cwd?: string) {
  if (!hasWorkspace.value || !host.value || opening) return opening
  opening = (async () => {
    loading.value = true
    error.value = ''
    try {
      // The host is hidden while there is no active tab. Make it measurable
      // before fitting, otherwise the first PTY starts at xterm's fallback
      // 30×24 and receives an immediate resize/SIGWINCH after its prompt.
      await nextTick()
      await ensureTerminal()
      await settleTerminalLayout()
      const { cols, rows } = terminalSize()
      await consoleStore.create(props.workspaceRoot, props.conversationId, cols, rows, cwd)
      await renderActiveTab()
      await syncSize()
      terminal?.focus()
    } catch (cause) {
      showError(cause)
    } finally {
      loading.value = false
      opening = null
    }
  })()
  return opening
}

async function renderActiveTab() {
  if (!terminal) return
  // activeTab is already scoped to this panel's workspace + conversation.
  const tab = activeTab.value
  if (!tab) {
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
  if (renderedSessionId !== tab.id || renderedOutputLength > tab.output.length) {
    terminal.write('\x1b[2J\x1b[3J\x1b[H')
    renderedSessionId = tab.id
    renderedOutputLength = 0
  }
  const pending = tab.output.slice(renderedOutputLength)
  if (!pending) return
  terminal.write(pending)
  renderedOutputLength = tab.output.length
}

async function selectTab(id: string) {
  consoleStore.select(id)
  await nextTick()
  await renderActiveTab()
  await syncSize()
  terminal?.focus()
}

async function restartActiveTab() {
  const tab = activeTab.value
  if (!tab) return
  await consoleStore.close(tab.id)
  await createTab(tab.cwd)
}

async function closeTab(id: string) {
  error.value = ''
  try {
    await consoleStore.close(id)
    await nextTick()
    await renderActiveTab()
  } catch (cause) {
    showError(cause)
  }
}

async function copyPath(tab: WorkspaceConsoleTab) {
  try {
    await navigator.clipboard.writeText(tab.cwd)
  } catch (cause) {
    showError(cause)
  } finally {
    closeContextMenu()
  }
}

watch(() => activeTab.value?.output, () => { void renderActiveTab() })
watch(() => consoleStore.activeSessionId, () => { void renderActiveTab() })
watch(contextMenu, menu => {
  if (menu) installOutsideMenuListeners()
  else removeOutsideMenuListeners?.()
})
watch(() => props.workspaceRoot, (newRoot) => {
  if (newRoot && props.active) {
    void createTab()
  }
})
watch(() => props.conversationId, async () => {
  // A conversation can already own tabs; pick the first one so the panel
  // shows its Shell instead of the empty state.
  const tabs = workspaceTabs.value
  if (tabs.length > 0 && !tabs.some(tab => tab.id === consoleStore.activeSessionId)) {
    consoleStore.select(tabs[0].id)
  }
  await renderActiveTab()
})

onMounted(() => {
  if (!props.active) return
  void (async () => {
    await ensureTerminal()
    // Entering the top-level Terminal view should always present a usable Shell.
    // Existing sessions remain available; create the initial one only when this
    // workspace has no selected session.
    if (activeTab.value?.workspaceRoot !== props.workspaceRoot) {
      await createTab()
    } else {
      await renderActiveTab()
      await syncSize()
    }
  })()
})

onBeforeUnmount(() => {
  removeSelectionGuard?.()
  removeSelectionGuard = null
  removeViewportScrollFallback()
  removeOutsideMenuListeners?.()
  removeOutsideMenuListeners = null
  imeGuard?.detach()
  imeGuard = null
  resizeObserver?.disconnect()
  resizeObserver = null
  themeObserver?.disconnect()
  themeObserver = null
  terminal?.dispose()
  terminal = null
  fitAddon = null
})
</script>

<template>
  <section class="terminal-panel flex min-h-0 flex-1 flex-col bg-card text-foreground" @click.self="closeContextMenu">
    <header class="console-chrome flex h-8 shrink-0 items-center gap-1 border-b border-border px-2 text-[11px]">
      <SquareTerminal class="h-3.5 w-3.5 shrink-0 text-accent" />
      <div class="console-tabs min-w-0 flex-1" role="tablist" aria-label="终端标签">
        <button
          v-for="tab in workspaceTabs"
          :key="tab.id"
          type="button"
          role="tab"
          class="console-tab"
          :class="consoleStore.activeSessionId === tab.id && 'is-active'"
          :aria-selected="consoleStore.activeSessionId === tab.id"
          :title="tab.cwd"
          @mousedown.left.prevent="selectTab(tab.id)"
          @contextmenu.prevent="openContextMenu($event, tab)"
        >
          <span class="truncate">{{ tab.label }}</span>
          <span v-if="tab.exited" class="text-amber-300">•</span>
          <X class="h-3 w-3 shrink-0 opacity-60 hover:opacity-100" @click.stop="closeTab(tab.id)" />
        </button>
        <button
          v-if="hasWorkspace"
          type="button"
          class="console-new-tab"
          title="新建独立 Shell"
          aria-label="新建独立 Shell"
          :disabled="loading"
          @click="createTab()"
        ><FolderPlus class="h-3.5 w-3.5" /></button>
      </div>
      <span v-if="loading" class="flex items-center gap-1 text-slate-400"><Loader2 class="h-3 w-3 animate-spin" />启动中</span>
      <button type="button" class="terminal-action" title="重启当前 Shell" :disabled="loading || !activeTab" @click="restartActiveTab"><RotateCcw class="h-3.5 w-3.5" /></button>
      <button type="button" class="terminal-action" title="结束当前 Shell" :disabled="loading || !activeTab" @click="activeTab && closeTab(activeTab.id)"><X class="h-3.5 w-3.5" /></button>
    </header>
    <div
      v-if="contextMenu"
      class="console-context-menu"
      role="menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
    >
      <button type="button" role="menuitem" @click="copyPath(contextMenu.tab)"><Copy class="h-3.5 w-3.5" />复制完整路径</button>
      <button type="button" role="menuitem" @click="closeTab(contextMenu.tab.id); closeContextMenu()"><X class="h-3.5 w-3.5" />关闭 Shell</button>
    </div>
    <p v-if="!hasWorkspace" class="m-auto max-w-56 text-center text-xs text-slate-400">请先在输入区选择项目目录，再启动调试终端。</p>
    <div v-else-if="!activeTab && !loading" class="m-auto flex flex-col items-center gap-3 text-center text-xs text-slate-400">
      <p>新建一个独立 Shell；每个标签有自己的目录、环境和前台进程。</p>
      <button type="button" class="console-create-first" @click="createTab()"><FolderPlus class="h-3.5 w-3.5" />新建终端</button>
    </div>
    <p v-if="error" class="absolute inset-x-3 top-11 z-10 rounded border border-red-400/40 bg-red-950/90 p-2 text-xs text-red-200">{{ error }}</p>
    <!-- Keep xterm mounted for cached output, but never show its canvas when
         the last Shell has been closed: the empty-state would otherwise render
         above a still-visible terminal buffer/prompt. -->
    <div
      v-show="hasWorkspace && (hasActiveTab || loading)"
      ref="host"
      class="terminal-host min-h-0 flex-1 p-2"
      data-workspace-terminal
    />
  </section>
</template>

<style scoped>
.terminal-panel :deep(.xterm) { height: 100%; }
.terminal-panel :deep(.xterm-screen) { user-select: none; }
.terminal-host { scrollbar-gutter: stable; user-select: none; }
/*
 * WKWebView / Safari: xterm's helper textarea defaults to opacity:0 and
 * z-index:-5 (css + runtime inline). WebKit will not open a system IME for
 * that "invisible" field, so Chinese composition never starts (Chromium is
 * fine). Keep the field engine-visible but visually transparent; xterm still
 * positions/sizes it under the cursor.
 */
.terminal-panel :deep(.xterm-helper-textarea) {
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
.terminal-panel :deep(.xterm-viewport) { overflow-y: auto !important; overscroll-behavior: contain; }
.console-chrome,
.console-tabs,
.console-tab,
.console-new-tab,
.terminal-action { @apply select-none; }
.console-tabs { @apply flex min-w-0 items-center gap-1 overflow-x-auto; }
.console-tab { @apply flex max-w-32 items-center gap-1 rounded px-1.5 py-1 text-muted; }
.console-tab:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-tab.is-active { background: hsl(var(--accent-muted)); color: hsl(var(--accent)); }
.console-new-tab, .terminal-action { @apply shrink-0 rounded p-1 text-muted disabled:cursor-not-allowed disabled:opacity-40; }
.console-new-tab:hover, .terminal-action:hover { background: hsl(var(--hover)); color: hsl(var(--foreground)); }
.console-context-menu { @apply fixed z-20 min-w-36 rounded border p-1 shadow-lg; border-color: hsl(var(--border)); background: hsl(var(--card)); transform: translateY(2px); }
.console-context-menu button { @apply flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-foreground; }
.console-context-menu button:hover { background: hsl(var(--hover)); }
.console-create-first { @apply flex items-center gap-1 rounded border px-2 py-1.5 text-foreground; border-color: hsl(var(--border)); }
.console-create-first:hover { background: hsl(var(--hover)); }
</style>
