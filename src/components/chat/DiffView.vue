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

/** Line number to render for a hidden line at collapse index ci, offset hi. */
function hiddenLineNum(ci: number, hi: number): number {
  const start = lineInfo.value.hiddenStarts.get(ci)
  return (start ?? 1) + hi
}
</script>

<template>
  <div class="diff-view">
    <div class="overflow-x-auto max-h-96">
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
              <span class="diff-num">{{ hiddenLineNum(idx, hi) }}</span>
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
            <span class="diff-num">{{ lineInfo.nums[idx] ?? '\u00A0' }}</span>
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
.diff-view {
  @apply rounded-lg overflow-hidden;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: #1e1e1e;
}

.diff-table {
  min-width: 100%;
}

/* ── Row ── */
.diff-row {
  display: flex;
  align-items: stretch;
  font-size: 13px;
  line-height: 1.6;
  font-family: 'JetBrains Mono', 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  min-height: 1.6em;
}

/* ── Line number gutter ── */
.diff-num {
  flex: 0 0 auto;
  width: 3ch;
  min-width: 3ch;
  text-align: right;
  padding: 0 6px 0 4px;
  user-select: none;
  @apply text-neutral-500 text-[12px];
  background: #252526;
  border-right: 1px solid rgba(255, 255, 255, 0.04);
}

/* ── Left colored bar ── */
.diff-bar {
  flex: 0 0 auto;
  width: 3px;
  min-width: 3px;
}

/* ── Code content ── */
.diff-text {
  flex: 1;
  padding: 0 12px;
  white-space: pre;
  tab-size: 2;
  overflow-x: auto;
}

/* ── Unchanged ── */
.diff-row.unchanged .diff-text {
  @apply text-neutral-400;
}

/* ── Removed (red) ── */
.diff-row.del {
  background: #3c1e1e;
}
.diff-row.del .diff-bar {
  background: #f14c4c;
}
.diff-row.del .diff-text {
  color: #d4bfbf;
}

/* ── Added (green) ── */
.diff-row.ins {
  background: #1e3c1e;
}
.diff-row.ins .diff-bar {
  background: #4ec94e;
}
.diff-row.ins .diff-text {
  color: #bfd4bf;
}

/* ── Collapse marker ── */
.diff-collapse {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 12px;
  cursor: pointer;
  user-select: none;
  @apply text-neutral-500 text-[12px] font-mono;
  background: #252526;
  border-top: 1px solid rgba(255, 255, 255, 0.04);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
}
.diff-collapse:hover {
  background: #2d2d2d;
}
.diff-collapse-icon {
  @apply text-neutral-400 text-[10px];
}
.diff-collapse-text {
  letter-spacing: 0.02em;
}

/* ── Footer ── */
.diff-footer {
  display: flex;
  gap: 16px;
  @apply text-[12px] px-3 py-1.5;
  background: #252526;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.diff-stat-diff {
  @apply text-green-400 font-medium;
}
.diff-stat-del {
  @apply text-red-400;
}
.num-label {
  @apply text-neutral-500 font-normal;
}
</style>
