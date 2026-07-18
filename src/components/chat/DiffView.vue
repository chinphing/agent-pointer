<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  oldContent: string
  newContent: string
}>()

interface DiffLine {
  type: 'unchanged' | 'del' | 'ins'
  text: string
}

function simpleLineDiff(oldText: string, newText: string): DiffLine[] {
  const oldLines = oldText.split('\n')
  const newLines = newText.split('\n')
  const lines: DiffLine[] = []

  // Build LCS table
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

  // Backtrack to build diff
  let i = m, j = n
  const stack: DiffLine[] = []
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

  while (stack.length) lines.push(stack.pop()!)
  return lines
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
  <div class="diff-view text-[13px] leading-6 font-mono">
    <div class="overflow-x-auto">
      <div
        v-for="(line, idx) in diffLines"
        :key="idx"
        class="diff-line"
        :class="line.type"
      ><span class="line-text">{{ line.text || '&nbsp;' }}</span></div>
    </div>
    <div v-if="stats.adds || stats.dels" class="diff-footer">
      +{{ stats.adds }} -{{ stats.dels }}
    </div>
  </div>
</template>

<style scoped>
.diff-view {
  @apply bg-neutral-950/50 rounded-lg overflow-hidden border border-white/[0.06];
}

.diff-line {
  padding: 0 12px;
  min-height: 1.5em;
  white-space: pre;
  tab-size: 2;
}

.diff-line.unchanged {
  @apply text-neutral-500;
}

.diff-line.del {
  @apply text-red-400;
  border-left: 3px solid theme('colors.red.600');
  background: theme('colors.red.950 / 25%');
}

.diff-line.ins {
  @apply text-green-400;
  border-left: 3px solid theme('colors.green.600');
  background: theme('colors.green.950 / 25%');
}

.diff-footer {
  @apply text-[11px] text-neutral-500 px-3 py-1 border-t border-white/[0.06] text-right;
}
</style>
