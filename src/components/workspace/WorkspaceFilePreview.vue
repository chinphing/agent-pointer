<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  ArrowDown,
  ArrowUp,
  Check,
  Copy,
  FileWarning,
  Search,
  WrapText,
  X
} from 'lucide-vue-next'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { WorkspaceFilePreview } from '../../lib/api'
import { parseMarkdown } from '../../lib/markdownConfig'
import { useMarkdownCharts } from '../../composables/useMarkdownCharts'
import {
  clearSearchTextMarks,
  highlightSearchText
} from '../../lib/sidebarSearchTextHighlight'
import {
  filePreviewSearchParts,
  findFilePreviewMatches,
  tokenizeCodeLine
} from '../../lib/workspaceFilePreview'

const FILE_PREVIEW_SEARCH_MARK_CLASS = 'file-preview-search-mark'

const props = defineProps<{
  preview: WorkspaceFilePreview
  absolutePath: string
}>()
const emit = defineEmits<{
  (e: 'open-reference', href: string): void
}>()

const rootElement = ref<HTMLElement | null>(null)
const searchInput = ref<HTMLInputElement | null>(null)
const markdownRoot = ref<HTMLElement | null>(null)

const wrapLines = ref(false)
const copied = ref(false)
const markdownMode = ref<'source' | 'preview'>('preview')
const searchOpen = ref(false)
const searchQuery = ref('')
const activeMatchIndex = ref(0)
const markdownMatchCount = ref(0)

useMarkdownCharts(markdownRoot, () => `${markdownMode.value}\n${props.preview.content ?? ''}`)

const IMAGE_EXTS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'])
const ext = computed(() => (props.absolutePath.split('.').pop()?.toLowerCase() || ''))
const isImage = computed(() => IMAGE_EXTS.has(ext.value))
const isPdf = computed(() => ext.value === 'pdf')
const isMarkdown = computed(() => ext.value === 'md')
const mediaUrl = computed(() => convertFileSrc(props.absolutePath))
const content = computed(() => props.preview.content ?? '')
const lines = computed(() => content.value.split('\n'))
const canSearch = computed(() => !props.preview.binary && !isImage.value && !isPdf.value)
const showingMarkdownPreview = computed(() => isMarkdown.value && markdownMode.value === 'preview')
const searchMatches = computed(() =>
  canSearch.value && !showingMarkdownPreview.value
    ? findFilePreviewMatches(content.value, searchQuery.value)
    : []
)
const language = computed(() => {
  const ext = props.preview.path.split('.').pop()?.toLowerCase() || ''
  const names: Record<string, string> = {
    ts: 'TypeScript', tsx: 'TSX', js: 'JavaScript', jsx: 'JSX', vue: 'Vue',
    rs: 'Rust', py: 'Python', go: 'Go', java: 'Java', kt: 'Kotlin', swift: 'Swift',
    json: 'JSON', jsonc: 'JSONC', toml: 'TOML', yaml: 'YAML', yml: 'YAML',
    md: 'Markdown', css: 'CSS', scss: 'SCSS', html: 'HTML', xml: 'XML',
    sh: 'Shell', sql: 'SQL', txt: 'Text'
  }
  return names[ext] || (ext ? ext.toUpperCase() : 'Text')
})
const sizeLabel = computed(() => {
  const bytes = props.preview.sizeBytes
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
})
const matchTotal = computed(() =>
  showingMarkdownPreview.value ? markdownMatchCount.value : searchMatches.value.length
)
const matchCountLabel = computed(() => {
  if (!searchQuery.value.trim()) return '0/0'
  if (!matchTotal.value) return '0/0'
  return `${activeMatchIndex.value + 1}/${matchTotal.value}`
})
const hasSearchMatches = computed(() => Boolean(searchQuery.value.trim()) && matchTotal.value > 0)

async function copyPath() {
  await navigator.clipboard.writeText(props.absolutePath)
  copied.value = true
  window.setTimeout(() => { copied.value = false }, 1200)
}

function openMarkdownReference(event: MouseEvent) {
  const target = event.target
  const root = event.currentTarget
  if (!(target instanceof Element) || !(root instanceof HTMLElement)) return
  const anchor = target.closest('a')
  if (!anchor || !root.contains(anchor)) return
  const href = anchor.getAttribute('href')?.trim() ?? ''
  event.preventDefault()
  event.stopPropagation()
  if (!href) {
    console.warn('[WorkspaceFilePreview] Markdown link has no destination')
    return
  }

  if (href.startsWith('#')) {
    let id = href.slice(1)
    try {
      id = decodeURIComponent(id)
    } catch {
      console.warn('[WorkspaceFilePreview] Markdown anchor is malformed', href)
      return
    }
    const destination = id ? root.querySelector(`#${CSS.escape(id)}`) as HTMLElement | null : null
    if (destination) destination.scrollIntoView({ behavior: 'smooth', block: 'start' })
    else console.warn('[WorkspaceFilePreview] Markdown anchor not found', href)
    return
  }
  emit('open-reference', href)
}

function isFilePreviewFindTarget(event: KeyboardEvent): boolean {
  const root = rootElement.value
  if (!root) return false
  const panel = root.closest('[data-workspace-panel]')
  const active = document.activeElement
  // File tab is active: accept focus anywhere in the workspace panel, or the event target.
  if (panel) {
    if (active instanceof Node && panel.contains(active)) return true
    if (event.target instanceof Node && panel.contains(event.target)) return true
  }
  return active instanceof Node && root.contains(active)
}

function openSearch() {
  if (!canSearch.value) {
    console.warn('[WorkspaceFilePreview] Find is unavailable for this preview')
    return
  }
  searchOpen.value = true
  void nextTick(() => {
    searchInput.value?.focus()
    searchInput.value?.select()
  })
}

function closeSearch() {
  searchOpen.value = false
  searchQuery.value = ''
  activeMatchIndex.value = 0
  clearMarkdownSearchMarks()
}

function clearMarkdownSearchMarks() {
  if (markdownRoot.value) clearSearchTextMarks(markdownRoot.value, FILE_PREVIEW_SEARCH_MARK_CLASS)
  markdownMatchCount.value = 0
}

function markdownMatchElements(): HTMLElement[] {
  if (!markdownRoot.value) return []
  return Array.from(markdownRoot.value.querySelectorAll<HTMLElement>(`.${FILE_PREVIEW_SEARCH_MARK_CLASS}`))
}

function applyMarkdownSearchHighlights() {
  const root = markdownRoot.value
  if (!root || !showingMarkdownPreview.value || !searchOpen.value) return
  const query = searchQuery.value.trim()
  if (!query) {
    clearSearchTextMarks(root, FILE_PREVIEW_SEARCH_MARK_CLASS)
    activeMatchIndex.value = 0
    return
  }
  const result = highlightSearchText(root, query, { markClass: FILE_PREVIEW_SEARCH_MARK_CLASS })
  markdownMatchCount.value = result.count
  if (result.count === 0) {
    activeMatchIndex.value = 0
    console.info('[WorkspaceFilePreview] No find matches in Markdown preview')
    return
  }
  if (activeMatchIndex.value >= result.count) activeMatchIndex.value = 0
  syncMarkdownActiveMatch()
}

function syncMarkdownActiveMatch() {
  const marks = markdownMatchElements()
  marks.forEach((mark, index) => {
    mark.classList.toggle('is-active-match', index === activeMatchIndex.value)
  })
  marks[activeMatchIndex.value]?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function scrollToSourceMatch(index: number) {
  await nextTick()
  const target = rootElement.value?.querySelector<HTMLElement>(
    `[data-file-search-match="${index}"]`
  )
  target?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function stepMatch(direction: 1 | -1) {
  if (showingMarkdownPreview.value) {
    const marks = markdownMatchElements()
    if (!marks.length) return
    activeMatchIndex.value = (activeMatchIndex.value + direction + marks.length) % marks.length
    syncMarkdownActiveMatch()
    return
  }
  const matches = searchMatches.value
  if (!matches.length) return
  activeMatchIndex.value = (activeMatchIndex.value + direction + matches.length) % matches.length
  await scrollToSourceMatch(activeMatchIndex.value)
}

function onSearchKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    closeSearch()
  } else if (event.key === 'Enter') {
    event.preventDefault()
    void stepMatch(event.shiftKey ? -1 : 1)
  }
}

function onGlobalFindShortcut(event: KeyboardEvent) {
  if (event.key.toLocaleLowerCase() !== 'f' || (!event.metaKey && !event.ctrlKey)) return
  if (event.defaultPrevented || !canSearch.value || !isFilePreviewFindTarget(event)) return
  event.preventDefault()
  event.stopImmediatePropagation()
  openSearch()
}

function onGlobalEscape(event: KeyboardEvent) {
  if (event.key !== 'Escape' || !searchOpen.value) return
  if (!isFilePreviewFindTarget(event) && document.activeElement !== searchInput.value) return
  event.preventDefault()
  closeSearch()
}

watch(searchMatches, async matches => {
  if (showingMarkdownPreview.value) return
  if (!matches.length) {
    activeMatchIndex.value = 0
    return
  }
  if (activeMatchIndex.value >= matches.length) activeMatchIndex.value = 0
  if (searchOpen.value && searchQuery.value.trim()) await scrollToSourceMatch(activeMatchIndex.value)
})

watch(
  [searchQuery, showingMarkdownPreview, () => props.preview.content, searchOpen],
  async () => {
    if (!showingMarkdownPreview.value || !searchOpen.value) {
      clearMarkdownSearchMarks()
      return
    }
    await nextTick()
    applyMarkdownSearchHighlights()
  }
)

watch(
  () => props.preview.path,
  () => {
    if (searchOpen.value) closeSearch()
  }
)

watch(markdownMode, () => {
  activeMatchIndex.value = 0
})

onMounted(() => {
  window.addEventListener('keydown', onGlobalFindShortcut, true)
  window.addEventListener('keydown', onGlobalEscape)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onGlobalFindShortcut, true)
  window.removeEventListener('keydown', onGlobalEscape)
  clearMarkdownSearchMarks()
})
</script>

<template>
  <div ref="rootElement" class="file-preview" data-workspace-file-preview tabindex="-1">
    <div class="file-preview-toolbar">
      <span>{{ language }}</span>
      <span>{{ sizeLabel }}</span>
      <span v-if="preview.truncated" class="text-warning">仅显示前 1 MB</span>
      <span class="flex-1" />
      <div v-if="isMarkdown" class="file-preview-mode-switch" role="group" aria-label="Markdown 显示模式">
        <button
          type="button"
          :class="markdownMode === 'source' && 'is-active'"
          :aria-pressed="markdownMode === 'source'"
          @click="markdownMode = 'source'"
        >原文</button>
        <button
          type="button"
          :class="markdownMode === 'preview' && 'is-active'"
          :aria-pressed="markdownMode === 'preview'"
          @click="markdownMode = 'preview'"
        >预览</button>
      </div>
      <button
        v-if="canSearch"
        type="button"
        :class="searchOpen && 'is-active'"
        title="查找（⌘F / Ctrl+F）"
        @click="searchOpen ? closeSearch() : openSearch()"
      >
        <Search />
      </button>
      <button
        v-if="!isMarkdown || markdownMode === 'source'"
        type="button"
        :class="wrapLines && 'is-active'"
        title="切换自动换行"
        @click="wrapLines = !wrapLines"
      >
        <WrapText />
      </button>
      <button type="button" title="复制绝对路径" @click="copyPath">
        <Check v-if="copied" /><Copy v-else />
      </button>
    </div>

    <div
      v-if="searchOpen && canSearch"
      class="file-preview-search"
      role="search"
    >
      <Search class="file-preview-search-icon" aria-hidden="true" />
      <input
        ref="searchInput"
        v-model="searchQuery"
        type="search"
        placeholder="查找"
        aria-label="在当前文件中查找"
        @keydown="onSearchKeydown"
      />
      <span class="file-preview-search-count">{{ matchCountLabel }}</span>
      <button
        type="button"
        title="上一个匹配（Shift+Enter）"
        :disabled="!hasSearchMatches"
        @click="stepMatch(-1)"
      >
        <ArrowUp />
      </button>
      <button
        type="button"
        title="下一个匹配（Enter）"
        :disabled="!hasSearchMatches"
        @click="stepMatch(1)"
      >
        <ArrowDown />
      </button>
      <button type="button" title="关闭（Esc）" @click="closeSearch">
        <X />
      </button>
    </div>

    <div v-if="isImage" class="file-preview-media">
      <img :src="mediaUrl" :alt="preview.path" />
    </div>
    <div v-else-if="isPdf" class="file-preview-media">
      <iframe :src="mediaUrl" class="file-preview-iframe" />
    </div>
    <div v-else-if="preview.binary" class="file-preview-empty">
      <FileWarning class="w-5 h-5" />
      <strong>无法预览二进制文件</strong>
      <span>{{ preview.path }} · {{ sizeLabel }}</span>
    </div>
    <div
      v-else-if="showingMarkdownPreview"
      class="file-preview-scroll"
      @click.capture="openMarkdownReference"
    >
      <div
        ref="markdownRoot"
        class="file-preview-markdown md-body px-3 py-2"
        v-html="parseMarkdown(preview.content ?? '')"
      />
    </div>
    <div v-else class="file-preview-scroll">
      <div class="file-preview-code" :class="wrapLines && 'wrap-lines'">
        <div
          v-for="(line, index) in lines"
          :key="index"
          class="file-preview-row"
          :class="searchMatches.some(match => match.lineIndex === index) && 'has-match'"
        >
          <span class="file-preview-line-number">{{ index + 1 }}</span>
          <code>
            <template
              v-for="(part, partIndex) in (
                searchOpen && searchQuery.trim()
                  ? filePreviewSearchParts(line, index, searchMatches)
                  : [{ text: line || '\u00A0' }]
              )"
              :key="partIndex"
            >
              <mark
                v-if="part.matchIndex != null"
                class="file-preview-search-mark"
                :class="part.matchIndex === activeMatchIndex && 'is-active-match'"
                :data-file-search-match="part.matchIndex"
              >{{ part.text }}</mark>
              <template v-else>
                <span
                  v-for="(token, tokenIndex) in tokenizeCodeLine(part.text === '\u00A0' ? '' : part.text)"
                  :key="tokenIndex"
                  :class="`token-${token.kind}`"
                >{{ token.text }}</span>
              </template>
            </template>
          </code>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.file-preview { @apply h-full min-h-0 flex flex-col overflow-hidden rounded-lg border border-border bg-card; }
.file-preview-toolbar { @apply h-9 shrink-0 flex items-center gap-2 border-b border-border px-2 text-[11px] text-muted; }
.file-preview-toolbar button { @apply rounded p-1 text-muted hover:bg-hover hover:text-foreground; }
.file-preview-toolbar button.is-active { @apply bg-hover text-accent; }
.file-preview-toolbar button :deep(svg) { @apply w-3.5 h-3.5; }
.file-preview-mode-switch { @apply flex items-center rounded-md bg-hover/60 p-0.5; }
.file-preview-mode-switch button { @apply px-1.5 py-0.5 leading-none; }
.file-preview-mode-switch button.is-active { @apply bg-card text-foreground shadow-sm; }
.file-preview-search {
  @apply h-9 shrink-0 flex items-center gap-1 border-b border-border px-2;
}
.file-preview-search-icon { @apply w-3.5 h-3.5 shrink-0 text-muted; }
.file-preview-search input {
  @apply min-w-0 flex-1 bg-transparent px-1 py-1 text-xs text-foreground outline-none placeholder:text-muted;
}
.file-preview-search-count { @apply min-w-10 shrink-0 text-center text-[11px] tabular-nums text-muted; }
.file-preview-search button {
  @apply rounded p-1 text-muted hover:bg-hover hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30;
}
.file-preview-search button :deep(svg) { @apply w-3.5 h-3.5; }
.file-preview-scroll { @apply flex-1 min-h-0 overflow-auto; }
.file-preview-code { @apply min-w-full w-max py-1 font-mono text-xs; }
.file-preview-row { @apply flex min-h-[1.55rem] leading-[1.55rem]; }
.file-preview-row.has-match { background: hsl(48 96% 53% / 0.08); }
.file-preview-line-number { @apply sticky left-0 z-[1] w-12 shrink-0 select-none border-r border-border bg-card px-2 text-right text-[11px] text-muted; }
.file-preview-row.has-match .file-preview-line-number { background: hsl(48 96% 53% / 0.08); }
.file-preview-row code { @apply block px-3 text-foreground whitespace-pre; tab-size: 2; }
.file-preview-code.wrap-lines { @apply w-full min-w-0; }
.file-preview-code.wrap-lines .file-preview-row code { @apply min-w-0 flex-1 whitespace-pre-wrap break-words; }
.file-preview-search-mark,
:deep(.file-preview-search-mark) {
  color: inherit;
  background: hsl(48 96% 53% / 0.72);
  border-radius: 0.2rem;
  box-shadow: 0 0 0 1px hsl(48 96% 53% / 0.35);
  padding: 0 0.08em;
}
.file-preview-search-mark.is-active-match,
:deep(.file-preview-search-mark.is-active-match) {
  background: hsl(48 96% 53% / 0.95);
  box-shadow: 0 0 0 1px hsl(32 95% 44% / 0.8);
}
.token-comment { @apply text-neutral-500 italic; }
.token-string { color: #ce9178; }
.token-number { color: #b5cea8; }
.token-keyword { color: #569cd6; font-weight: 500; }
html.light .token-string { color: #a31515; }
html.light .token-number { color: #098658; }
html.light .token-keyword { color: #0000ff; }
.file-preview-empty { @apply flex-1 flex flex-col items-center justify-center gap-2 p-5 text-center text-xs text-muted; }
.file-preview-empty strong { @apply text-foreground; }
.file-preview-media { @apply flex-1 min-h-0 flex items-center justify-center overflow-auto; }
.file-preview-media img { @apply max-w-full max-h-full object-contain; }
.file-preview-iframe { @apply w-full h-full border-0; }
</style>
