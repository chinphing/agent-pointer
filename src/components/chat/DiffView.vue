<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  oldContent: string
  newContent: string
}>()

interface DiffLine {
  type: 'unchanged' | 'del' | 'ins'
  text: string
  num: number // display line number
}

function simpleLineDiff(oldText: string, newText: string): DiffLine[] {
  const oldLines = oldText.split('\n')
  const newLines = newText.split('\n')
  const raw: { type: 'unchanged' | 'del' | 'ins'; text: string }[] = []

  const m = oldLines.length
  const n = newLines.length
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0))
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (oldLines[i - 1] === newLines[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1] + 1
      } else {
        dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1])
      }
    }
  }

  let i = m, j = n
  const stack: { type: 'unchanged' | 'del' | 'ins'; text: string }[] = []
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      stack.push({ type: 'unchanged', text: oldLines[i - 1] })
      i--; j--
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      stack.push({ type: 'ins', text: newLines[j - 1] })
      j--
    } else {
      stack.push({ type: 'del', text: oldLines[i - 1] })
      i--
    }
  }
  while (stack.length) raw.push(stack.pop()!)

  // Assign line numbers sequentially
  let num = 0
  const result: DiffLine[] = []
  for (const line of raw) {
    num++
    result.push({ ...line, num })
  }
  return result
}

const diffLines = computed(() => simpleLineDiff(props.oldContent, props.newContent))

const stats = computed(() => {
  let adds = 0, dels = 0
  for (const l of diffLines.value) {
    if (l.type === 'ins') adds++
    else if (l.type === 'del') dels++
  }
  return { adds, dels }
})
</script>

<template>
  <div class="diff-view">
    <div class="overflow-x-auto max-h-96">
      <div class="diff-table">
        <div
          v-for="line in diffLines"
          :key="line.num"
          class="diff-row"
          :class="line.type"
        >
          <span class="diff-num">{{ line.num }}</span>
          <span class="diff-bar"></span>
          <span class="diff-text">{{ line.text }}</span>
        </div>
      </div>
    </div>
    <div v-if="stats.adds || stats.dels" class="diff-footer">
      <span class="diff-stat-diff">+{{ stats.adds }}<span class="num-label"> 新增</span></span>
      <span class="diff-stat-diff diff-stat-del">-{{ stats.dels }}<span class="num-label"> 删除</span></span>
    </div>
  </div>
</template>

<style scoped>
.diff-view {
  @apply rounded-lg overflow-hidden;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: #1e1e1e; /* VSCode editor bg */
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
  width: 3.5ch;
  min-width: 3.5ch;
  text-align: right;
  padding: 0 8px 0 4px;
  user-select: none;
  @apply text-neutral-500 text-[12px];
  background: #252526; /* VSCode gutter bg */
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
  background: #3c1e1e; /* VSCode red diff bg */
}
.diff-row.del .diff-bar {
  background: #f14c4c;
}
.diff-row.del .diff-text {
  color: #d4bfbf;
}

/* ── Added (green) ── */
.diff-row.ins {
  background: #1e3c1e; /* VSCode green diff bg */
}
.diff-row.ins .diff-bar {
  background: #4ec94e;
}
.diff-row.ins .diff-text {
  color: #bfd4bf;
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
