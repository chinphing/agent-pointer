<script setup lang="ts">
import { ref, computed } from 'vue'

export type DiffLine = {
  type: 'unchanged' | 'del' | 'ins' | 'collapse'
  text: string
  hidden?: string[]
}

const props = defineProps<{
  diffLines: DiffLine[]
  diffStats?: { adds: number; dels: number }
}>()

// Track which collapse sections are expanded
const expanded = ref<Set<number>>(new Set())

function toggleCollapse(idx: number) {
  const s = new Set(expanded.value)
  if (s.has(idx)) s.delete(idx)
  else s.add(idx)
  expanded.value = s
}

// Build line numbers for every position including hidden lines inside collapses.
// idx → old‐file line number (1‑based), or null for ins lines / collapse markers.
function buildLineNums(lines: DiffLine[]) {
  const nums: (number | null)[] = []
  // hiddenStarts[idx] = first old‐line number of the hidden block at collapse line idx
  const hiddenStarts = new Map<number, number>()
  let oldLine = 1
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i]
    if (l.type === 'ins') {
      nums.push(null)
    } else if (l.type === 'collapse') {
      nums.push(null)
      hiddenStarts.set(i, oldLine)
      oldLine += l.hidden?.length || 0
    } else {
      nums.push(oldLine++)
    }
  }
  return { nums, hiddenStarts }
}

const lineInfo = computed(() => buildLineNums(props.diffLines))

/** CSS width in ch units for the line-number gutter, based on max line number. */
const numWidth = computed(() => {
  let max = 0
  for (const n of lineInfo.value.nums) {
    if (n != null && n > max) max = n
  }
  // Also account for hidden lines inside collapsed sections
  for (const [ci, start] of lineInfo.value.hiddenStarts) {
    const line = props.diffLines[ci]
    const count = line?.hidden?.length || 0
    if (count > 0) {
      const end = start + count - 1
      if (end > max) max = end
    }
  }
  const digits = max === 0 ? 1 : String(max).length
  return `${Math.max(digits, 3)}ch`
})

/** Line number to render for a hidden line at collapse index ci, offset hi. */
function hiddenLineNum(ci: number, hi: number): number {
  const start = lineInfo.value.hiddenStarts.get(ci)
  return (start ?? 1) + hi
}
</script>

<template>
  <div class="diff-view">
    <div class="overflow-x-hidden max-h-96">
      <div class="diff-table">
        <template v-for="(line, idx) in diffLines" :key="idx">
          <!-- Collapse marker -->
          <div
            v-if="line.type === 'collapse'"
            class="diff-collapse"
            @click="toggleCollapse(idx)"
          >
            <span class="diff-collapse-icon">{{ expanded.has(idx) ? '▾' : '▸' }}</span>
            <span class="diff-collapse-text">┄ +{{ line.text }} 行 ┄</span>
          </div>

          <!-- Hidden lines (only rendered when expanded) -->
          <template v-if="line.type === 'collapse' && expanded.has(idx)">
            <div
              v-for="(h, hi) in line.hidden"
              :key="'h-' + hi"
              class="diff-row unchanged"
            >
              <span class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ hiddenLineNum(idx, hi) }}</span>
              <span class="diff-bar"></span>
              <span class="diff-text">{{ h }}</span>
            </div>
          </template>

          <!-- Regular line -->
          <div
            v-if="line.type !== 'collapse'"
            class="diff-row"
            :class="line.type"
          >
            <span class="diff-num" :style="{ width: numWidth, minWidth: numWidth }">{{ lineInfo.nums[idx] ?? '\u00A0' }}</span>
            <span class="diff-bar"></span>
            <span class="diff-text">{{ line.text }}</span>
          </div>
        </template>
      </div>
    </div>
    <div
      v-if="diffStats && (diffStats.adds || diffStats.dels)"
      class="diff-footer"
    >
      <span class="diff-stat-diff">+{{ diffStats.adds }}<span class="num-label"> 新增</span></span>
      <span class="diff-stat-diff diff-stat-del">-{{ diffStats.dels }}<span class="num-label"> 删除</span></span>
    </div>
  </div>
</template>

<style scoped>
/* ── Base layout ── */
.diff-view {
  @apply rounded-lg overflow-hidden;
  border: 1px solid;
}
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
  flex: 0 0 auto;
  width: 3ch;
  min-width: 3ch;
  text-align: right;
  padding: 0 6px 0 4px;
  user-select: none;
  @apply text-neutral-500 text-[12px];
  border-right: 1px solid;
}
.diff-bar { flex: 0 0 auto; width: 3px; min-width: 3px; }
.diff-text {
  flex: 1;
  padding: 0 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  tab-size: 2;
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
