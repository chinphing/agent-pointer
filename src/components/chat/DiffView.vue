<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  oldContent: string
  newContent: string
  fileName?: string
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

const hasChanges = computed(() => stats.value.adds > 0 || stats.value.dels > 0)
</script>

<template>
  <div class="diff-view">
    <!-- Stats bar -->
    <div v-if="hasChanges" class="diff-stats text-[11px] text-muted px-3 py-1 border-b border-white/5 flex items-center gap-3">
      <span class="text-green-400 font-medium">+{{ stats.adds }}</span>
      <span class="text-red-400 font-medium">-{{ stats.dels }}</span>
    </div>
    <!-- Diff lines -->
    <div class="overflow-x-auto max-h-80">
      <pre class="text-[12px] font-mono leading-relaxed"><code><template v-for="(line, idx) in diffLines" :key="idx"><span
  :class="line.type === 'del' ? 'diff-del' : line.type === 'ins' ? 'diff-ins' : 'diff-unchanged'"
  class="block min-h-[1.4em]"
>{{ line.text || ' ' }}</span>
</template></code></pre>
    </div>
  </div>
</template>

<style scoped>
.diff-view {
  @apply bg-black/40 rounded-lg border border-white/5 overflow-hidden;
}

.diff-del {
  @apply bg-red-900/30 text-red-200;
}

.diff-ins {
  @apply bg-green-900/30 text-green-200;
}

.diff-unchanged {
  @apply text-slate-400;
}
</style>
