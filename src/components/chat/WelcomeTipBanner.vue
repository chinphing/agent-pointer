<script setup lang="ts">
import { Info } from 'lucide-vue-next'
import type { WelcomeTip } from '../../lib/webBranding'

defineProps<{
  tip: WelcomeTip
}>()
</script>

<template>
  <aside
    class="flex w-full items-start gap-2.5 rounded-2xl border border-accent/20 bg-accent-muted px-3.5 py-3 text-left"
    role="note"
  >
    <span
      class="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-lg bg-accent text-accent-foreground"
      aria-hidden="true"
    >
      <Info class="h-3.5 w-3.5" />
    </span>
    <div class="min-w-0 flex-1">
      <p
        v-if="tip.title"
        class="text-sm font-semibold leading-snug text-foreground"
      >{{ tip.title }}</p>
      <p
        v-if="tip.body"
        class="mt-1 text-[13px] leading-relaxed text-muted"
        :class="tip.title ? '' : 'mt-0'"
      >{{ tip.body }}</p>
    </div>
  </aside>
</template>
