<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Copy, FolderPlus, Loader2, RotateCcw, SquareTerminal, X } from 'lucide-vue-next'
import { useConsoleStore, type WorkspaceConsoleTab } from '../../stores/console'

const props = defineProps<{ workspaceRoot: string; active: boolean }>()

const consoleStore = useConsoleStore()
const host = ref<HTMLElement | null>(null)
const loading = ref(false)
const error = ref('')
const contextMenu = ref<{ tab: WorkspaceConsoleTab; x: number; y: number } | null>(null)
const hasActiveTab = computed(() => !!activeTab.value)
const hasWorkspace = computed(() => !!props.workspaceRoot.trim())
const activeTab = computed(() => consoleStore.activeTab)
let terminal: import('@xterm/xterm').Terminal | null = null
let fitAddon: import('@xterm/addon-fit').FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null
let opening: Promise<void> | null = null
let renderedSessionId = ''
let renderedOutputLength = 0
let removeOutsideMenuListeners: (() => void) | null = null
let pointerDown: { x: number; y: number } | null = null
// A 4px threshold is smaller than one terminal character cell, so ordinary
// click jitter can cross a cell boundary and make xterm retain a selection.
const SELECTION_DRAG_THRESHOLD_PX = 12

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
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-fit'),
    import('@xterm/xterm/css/xterm.css')
  ])
  terminal = new Terminal({
    // Keep the pre-fit screen within the panel's minimum width. FitAddon
    // replaces this conservative value once the async-mounted view settles.
    cols: 30,
    cursorBlink: true,
    convertEol: false,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
    fontSize: 12,
    scrollback: 5_000,
    theme: terminalTheme()
  })
  fitAddon = new FitAddon()
  terminal.loadAddon(fitAddon)
  terminal.open(host.value)
  terminal.onData(data => {
    const id = consoleStore.activeSessionId
    if (!id) return
    void consoleStore.write(id, data).catch(showError)
  })
  themeObserver = new MutationObserver(syncTerminalTheme)
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] })
  resizeObserver = new ResizeObserver(() => { void syncSize() })
  resizeObserver.observe(host.value)
  await nextTick()
  await settleTerminalLayout()
  fitAddon.fit()
}

function handleTerminalPointerDown(event: PointerEvent) {
  pointerDown = { x: event.clientX, y: event.clientY }
  // Clear a previous xterm selection before routing the click to the PTY.
  // This must happen on pointerdown: xterm's own mousedown handler starts a
  // new selection during the same gesture, so clearing only on pointerup
  // cannot distinguish a focus click with minor mouse movement from a drag.
  terminal?.clearSelection()
  // Mouse pointers are not implicitly captured. Capture the original xterm
  // target so a release outside the host still bubbles a pointerup here.
  // Capturing the target also leaves xterm's scrollbar capture untouched.
  const target = event.target as Element | null
  if (target?.setPointerCapture && !target.hasPointerCapture?.(event.pointerId)) {
    target.setPointerCapture(event.pointerId)
  }
}

function handleTerminalPointerUp(event: PointerEvent) {
  if (!pointerDown) return
  const distance = Math.hypot(event.clientX - pointerDown.x, event.clientY - pointerDown.y)
  pointerDown = null
  // Keep a clear intent boundary for drag-to-select. A terminal character is
  // roughly 7px wide at the configured font; 12px avoids treating click
  // jitter that crossed one cell as an intentional text selection.
  if (distance < SELECTION_DRAG_THRESHOLD_PX) terminal?.clearSelection()
}

function handleTerminalPointerCancel() {
  pointerDown = null
}

function closeContextMenu() {
  contextMenu.value = null
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
      await consoleStore.create(props.workspaceRoot, cols, rows, cwd)
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
  const tab = activeTab.value
  if (!tab) {
    // xterm 6.0.0 queues writes (parsed via setTimeout); clear() is
    // synchronous and does not drain the queue, so pending bytes from
    // the previous tab can be parsed into the view after the wipe.
    // Erase through the write queue so the wipe is ordered after any
    // pending writes.
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
watch(() => props.workspaceRoot, () => {
  // Console sessions are independent user-owned processes. Changing the active
  // workspace must not terminate a Shell from the previous workspace.
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
  removeOutsideMenuListeners?.()
  removeOutsideMenuListeners = null
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
    <header class="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2 text-[11px]">
      <SquareTerminal class="h-3.5 w-3.5 shrink-0 text-accent" />
      <div class="console-tabs min-w-0 flex-1" role="tablist" aria-label="终端标签">
        <button
          v-for="tab in consoleStore.tabs"
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
      @pointerdown="handleTerminalPointerDown"
      @pointerup="handleTerminalPointerUp"
      @pointercancel="handleTerminalPointerCancel"
    />
  </section>
</template>

<style scoped>
.terminal-panel :deep(.xterm) { height: 100%; }
.terminal-panel :deep(.xterm-screen) { user-select: none; }
.terminal-host { scrollbar-gutter: stable; user-select: none; }
.terminal-panel :deep(.xterm-viewport) { overflow-y: auto !important; }
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
