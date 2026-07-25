<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowDown, ArrowUp, ChevronDown, ChevronUp, Expand, Minimize2, Search, Space, WrapText, X } from 'lucide-vue-next'
import { findDiffChangeBlocks, findDiffMatches, showDiffWhitespace } from '../../lib/diffView'

export type DiffLine = {
  type: 'unchanged' | 'del' | 'ins' | 'collapse'
  text: string
  hidden?: string[]
  oldLine?: number
  newLine?: number
  hiddenOldLine?: number
  hiddenNewLine?: number
  isHunkHeader?: boolean
}

const props = withDefaults(defineProps<{
  diffLines: DiffLine[]
  diffStats?: { adds: number; dels: number }
  fillHeight?: boolean
  showGitLineNumbers?: boolean
}>(), {
  fillHeight: false,
  showGitLineNumbers: false
})

// Track which collapse sections are expanded
const expanded = ref<Set<number>>(new Set())
const rootElement = ref<HTMLElement | null>(null)
const scrollElement = ref<HTMLElement | null>(null)
const searchQuery = ref('')
const activeMatchIndex = ref(-1)
const activeBlockIndex = ref(-1)
const wrapLines = ref(true)
const showWhitespace = ref(false)
const maximized = ref(false)

const searchMatches = computed(() => findDiffMatches(props.diffLines, searchQuery.value))
const changeBlocks = computed(() => findDiffChangeBlocks(props.diffLines))
const activeMatchKey = computed(() => searchMatches.value[activeMatchIndex.value]?.key)

function lineText(text: string) {
  return showWhitespace.value ? showDiffWhitespace(text) : text
}

async function scrollToKey(key: string, parentIndex?: number) {
  if (parentIndex !== undefined && !expanded.value.has(parentIndex)) {
    expanded.value = new Set(expanded.value).add(parentIndex)
  }
  await nextTick()
  const target = rootElement.value?.querySelector<HTMLElement>(`[data-diff-key="${key}"]`)
  target?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

async function stepMatch(direction: 1 | -1) {
  const matches = searchMatches.value
  if (!matches.length) return
  activeMatchIndex.value = (activeMatchIndex.value + direction + matches.length) % matches.length
  const match = matches[activeMatchIndex.value]!
  await scrollToKey(match.key, match.parentIndex)
}

async function stepBlock(direction: 1 | -1) {
  const blocks = changeBlocks.value
  if (!blocks.length) return
  activeBlockIndex.value = (activeBlockIndex.value + direction + blocks.length) % blocks.length
  await scrollToKey(blocks[activeBlockIndex.value]!)
}

function toggleMaximized() {
  maximized.value = !maximized.value
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && maximized.value) maximized.value = false
}

watch(searchMatches, () => {
  activeMatchIndex.value = -1
})

onMounted(() => document.addEventListener('keydown', handleKeydown))
onBeforeUnmount(() => document.removeEventListener('keydown', handleKeydown))

function toggleCollapse(idx: number) {
  const s = new Set(expanded.value)
  if (s.has(idx)) s.delete(idx)
  else s.add(idx)
  expanded.value = s
}

// Legacy tool-call diffs do not carry Git positions. Keep their existing
// sequential gutter while workspace diffs use the parser-provided positions.
function buildLegacyLineNums(lines: DiffLine[]) {
  const nums: (number | null)[] = []
  const hiddenStarts = new Map<number, number>()
  let oldLine = 1
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (line.type === 'ins') nums.push(null)
    else if (line.type === 'collapse') {
      nums.push(null)
      hiddenStarts.set(i, oldLine)
      oldLine += line.hidden?.length || 0
    } else nums.push(oldLine++)
  }
  return { nums, hiddenStarts }
}

const lineInfo = computed(() => buildLegacyLineNums(props.diffLines))

function lineNumbers(line: DiffLine, idx: number): [number | null, number | null] {
  const legacy = lineInfo.value.nums[idx] ?? null
  if (!props.showGitLineNumbers) return [legacy, null]
  if (line.oldLine !== undefined || line.newLine !== undefined) {
    return [line.oldLine ?? null, line.newLine ?? null]
  }
  return [line.type === 'ins' ? null : legacy, line.type === 'del' ? null : legacy]
}

function hiddenLineNumbers(line: DiffLine, idx: number, hi: number): [number | null, number | null] {
  const start = lineInfo.value.hiddenStarts.get(idx) ?? 1
  if (!props.showGitLineNumbers) return [start + hi, null]
  if (line.hiddenOldLine !== undefined || line.hiddenNewLine !== undefined) {
    return [
      line.hiddenOldLine === undefined ? null : line.hiddenOldLine + hi,
      line.hiddenNewLine === undefined ? null : line.hiddenNewLine + hi
    ]
  }
  return [start + hi, start + hi]
}

/** Gutter width including both old and new Git line-number columns. */
const numWidth = computed(() => 'calc(3ch + 8px)')
</script>

<template>
  <div
    ref="rootElement"
    class="diff-view"
    :class="[
      fillHeight && 'diff-view-fill',
      maximized && 'diff-view-maximized',
      !wrapLines && 'diff-view-nowrap'
    ]"
  >
    <div v-if="fillHeight" class="diff-toolbar">
      <label class="diff-search">
        <Search />
        <input v-model="searchQuery" type="search" placeholder="搜索 Diff" @keydown.enter.prevent="stepMatch(1)" />
        <span v-if="searchQuery">{{ searchMatches.length ? `${activeMatchIndex + 1}/${searchMatches.length}` : '0/0' }}</span>
        <button v-if="searchQuery" type="button" title="清除搜索" @click="searchQuery = ''"><X /></button>
      </label>
      <button type="button" :disabled="!searchMatches.length" title="上一个匹配" @click="stepMatch(-1)"><ArrowUp /></button>
      <button type="button" :disabled="!searchMatches.length" title="下一个匹配" @click="stepMatch(1)"><ArrowDown /></button>
      <span class="diff-toolbar-divider" />
      <button type="button" :disabled="!changeBlocks.length" title="上一个变更块" @click="stepBlock(-1)"><ChevronUp /></button>
      <button type="button" :disabled="!changeBlocks.length" title="下一个变更块" @click="stepBlock(1)"><ChevronDown /></button>
      <button type="button" :class="wrapLines && 'is-active'" title="切换长行换行" @click="wrapLines = !wrapLines"><WrapText /></button>
      <button type="button" :class="showWhitespace && 'is-active'" title="显示空白字符" @click="showWhitespace = !showWhitespace"><Space /></button>
      <button type="button" :title="maximized ? '退出放大' : '放大 Diff'" @click="toggleMaximized">
        <Minimize2 v-if="maximized" /><Expand v-else />
      </button>
    </div>

    <div
      ref="scrollElement"
      :class="fillHeight ? 'diff-scroll flex-1 min-h-0 overflow-y-auto' : 'overflow-x-hidden max-h-96'"
    >
      <div class="diff-table">
        <template v-for="(line, idx) in diffLines" :key="idx">
          <div
            v-if="line.type === 'collapse'"
            class="diff-collapse"
            :class="line.isHunkHeader && 'diff-hunk-header'"
            @click="!line.isHunkHeader && toggleCollapse(idx)"
          >
            <span v-if="!line.isHunkHeader" class="diff-collapse-icon">{{ expanded.has(idx) ? '▾' : '▸' }}</span>
            <span class="diff-collapse-text">{{ line.isHunkHeader ? line.text : `┄ 展开 ${line.text} 行 ┄` }}</span>
          </div>

          <template v-if="line.type === 'collapse' && expanded.has(idx)">
            <div
              v-for="(h, hi) in line.hidden"
              :key="'h-' + hi"
              class="diff-row unchanged"
              :class="activeMatchKey === `hidden-${idx}-${hi}` && 'is-match'"
              :data-diff-key="`hidden-${idx}-${hi}`"
            >
              <span class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ hiddenLineNumbers(line, idx, hi)[0] ?? '\u00A0' }}</span>
              <span v-if="showGitLineNumbers" class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ hiddenLineNumbers(line, idx, hi)[1] ?? '\u00A0' }}</span>
              <span class="diff-bar"></span>
              <span class="diff-text">{{ lineText(h) }}</span>
            </div>
          </template>

          <div
            v-if="line.type !== 'collapse'"
            class="diff-row"
            :class="[line.type, activeMatchKey === `line-${idx}` && 'is-match']"
            :data-diff-key="`line-${idx}`"
          >
            <span class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ lineNumbers(line, idx)[0] ?? '\u00A0' }}</span>
            <span v-if="showGitLineNumbers" class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ lineNumbers(line, idx)[1] ?? '\u00A0' }}</span>
            <span class="diff-bar"></span>
            <span class="diff-text">{{ lineText(line.text) }}</span>
          </div>
        </template>
      </div>
    </div>
    <div v-if="diffStats && (diffStats.adds || diffStats.dels)" class="diff-footer">
      <span class="diff-stat-diff">+{{ diffStats.adds }}<span class="num-label"> 新增</span></span>
      <span class="diff-stat-diff diff-stat-del">-{{ diffStats.dels }}<span class="num-label"> 删除</span></span>
      <span v-if="fillHeight" class="ml-auto text-neutral-500">{{ changeBlocks.length }} 个变更块</span>
    </div>
  </div>
</template>

<style scoped>
/* ── Base layout ── */
.diff-view {
  @apply rounded-lg overflow-hidden;
  border: 1px solid;
}
.diff-view-fill {
  @apply h-full min-h-0 flex flex-col;
}
.diff-view-maximized {
  @apply fixed inset-4 z-[350] h-auto max-h-none rounded-xl shadow-2xl;
}
.diff-toolbar {
  @apply h-9 shrink-0 flex items-center gap-1 border-b px-1.5;
}
.diff-toolbar > button {
  @apply rounded p-1 text-neutral-500 hover:text-neutral-200 disabled:cursor-not-allowed disabled:opacity-30;
}
.diff-toolbar > button.is-active { @apply bg-white/10 text-blue-400; }
.diff-toolbar svg { @apply w-3.5 h-3.5; }
.diff-toolbar-divider { @apply h-4 w-px mx-0.5 bg-white/10; }
.diff-search { @apply min-w-0 flex-1 flex items-center gap-1 rounded border px-1.5 py-1 text-[11px]; }
.diff-search input { @apply min-w-0 flex-1 bg-transparent text-inherit outline-none; }
.diff-search > button { @apply shrink-0 text-neutral-500 hover:text-neutral-200; }
.diff-scroll { overflow-x: hidden; }
.diff-view-nowrap .diff-scroll { overflow-x: auto; }
.diff-view-nowrap .diff-table { width: max-content; }
.diff-view-nowrap .diff-text { white-space: pre; overflow-wrap: normal; }
.diff-row.is-match { outline: 1px solid rgba(250, 204, 21, 0.9); outline-offset: -1px; }
.diff-table { min-width: 100%; }
.diff-row {
  display: flex;
  align-items: stretch;
  font-size: 13px;
  line-height: 1.6;
  font-family: 'Cascadia Code', 'JetBrains Mono', 'Fira Code', 'SF Mono', 'Menlo', 'Consolas', 'DejaVu Sans Mono', 'Noto Sans Mono', 'Source Code Pro', 'Courier New', monospace;
  min-height: 1.6em;
}
.diff-num {
  box-sizing: border-box;
  flex: 0 0 auto;
  width: calc(3ch + 8px);
  min-width: calc(3ch + 8px);
  text-align: right;
  padding: 0 5px 0 3px;
  user-select: none;
  @apply text-neutral-500 text-[12px];
  border-right: 1px solid;
}
.diff-bar { flex: 0 0 auto; width: 3px; min-width: 3px; }
.diff-text {
  flex: 1;
  padding: 0 8px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  tab-size: 2;
  font-variant-ligatures: none;
}
.diff-collapse {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 12px;
  cursor: pointer;
  user-select: none;
  @apply text-neutral-500 text-[12px] font-mono;
  border-top: 1px solid;
  border-bottom: 1px solid;
}
.diff-collapse-icon { @apply text-neutral-400 text-[10px]; }
.diff-collapse-text { letter-spacing: 0.02em; }
.diff-footer {
  display: flex;
  gap: 16px;
  @apply text-[12px] px-3 py-1.5;
  border-top: 1px solid;
}
.diff-stat-diff { @apply text-green-500 font-medium; }
.diff-stat-del { @apply text-red-500; }
.num-label { @apply text-neutral-500 font-normal; }

/* ── Dark theme ── */
html.dark .diff-view {
  border-color: rgba(255, 255, 255, 0.06);
  background: #1e1e1e;
}
html.dark .diff-num {
  background: #252526;
  border-color: rgba(255, 255, 255, 0.04);
}
html.dark .diff-row.unchanged .diff-text { @apply text-neutral-400; }
html.dark .diff-row.del { background: #3c1e1e; }
html.dark .diff-row.del .diff-bar { background: #f14c4c; }
html.dark .diff-row.del .diff-text { color: #d4bfbf; }
html.dark .diff-row.ins { background: #1e3c1e; }
html.dark .diff-row.ins .diff-bar { background: #4ec94e; }
html.dark .diff-row.ins .diff-text { color: #bfd4bf; }
html.dark .diff-collapse {
  background: #252526;
  border-color: rgba(255, 255, 255, 0.04);
}
html.dark .diff-collapse:hover { background: #2d2d2d; }
html.dark .diff-footer {
  background: #252526;
  border-color: rgba(255, 255, 255, 0.06);
}

/* ── Light theme ── */
html.light .diff-view {
  border-color: rgba(0, 0, 0, 0.08);
  background: #ffffff;
}
html.light .diff-num {
  background: #f0f0f0;
  border-color: rgba(0, 0, 0, 0.06);
}
html.light .diff-row.unchanged .diff-text { @apply text-neutral-700; }
html.light .diff-row.del { background: #ffeef0; }
html.light .diff-row.del .diff-bar { background: #cb2431; }
html.light .diff-row.del .diff-text { color: #86181d; }
html.light .diff-row.ins { background: #e6ffed; }
html.light .diff-row.ins .diff-bar { background: #22863a; }
html.light .diff-row.ins .diff-text { color: #144620; }
html.light .diff-collapse {
  background: #f0f0f0;
  border-color: rgba(0, 0, 0, 0.06);
}
html.light .diff-collapse:hover { background: #e4e4e4; }
html.light .diff-footer {
  background: #f0f0f0;
  border-color: rgba(0, 0, 0, 0.08);
}
</style>
