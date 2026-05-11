<script setup lang="ts">
import { computed } from 'vue'
import type { AgentTrace } from '../../../../types/chat'
const props = defineProps<{
  /** XML `<thoughts>` — shown in the slot previously used for API reasoning. */
  xmlThoughts?: string
  agentTrace?: AgentTrace[]
}>()

/** Keep open after stream ends so long `<thoughts>` are visible (was auto-collapsed at 360+ chars). */
const thoughtsOpen = computed(() => {
  const t = props.xmlThoughts?.trim() ?? ''
  return t.length > 0
})

const traceOpen = computed(() => (props.agentTrace?.length ?? 0) > 0)
</script>

<template>
  <div v-if="xmlThoughts?.trim() || agentTrace?.length" class="space-y-2">
    <!-- thoughts 直接显示，不套框 -->
    <div
      v-if="xmlThoughts?.trim()"
      class="text-[12px] leading-relaxed text-slate-400/90 whitespace-pre-wrap"
    >{{ xmlThoughts }}</div>

    <details
      v-if="agentTrace?.length"
      class="rounded-xl border border-violet-500/25 bg-violet-500/5 overflow-hidden"
      :open="traceOpen"
    >
      <summary
        class="cursor-pointer select-none px-3 py-2 text-[11px] font-medium text-violet-200/90 hover:bg-violet-500/10 transition list-none flex items-center gap-2"
      >
        <span class="text-violet-300/80">多智能体轨迹</span>
        <span class="text-slate-500 font-normal">子 Agent 步骤与中间输出</span>
      </summary>
      <div class="border-t border-violet-500/15 px-3 py-2 space-y-2 text-[11px] text-slate-300">
        <div v-for="agent in agentTrace" :key="agent.id" class="rounded-lg bg-black/20 p-2">
          <div class="flex flex-wrap gap-2">
            <span class="text-slate-100">{{ agent.name }}</span>
            <span class="text-slate-500">{{ agent.status }}</span>
            <span v-if="agent.detail" class="text-slate-400">{{ agent.detail }}</span>
          </div>
          <pre
            v-if="agent.content"
            class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-md bg-black/30 p-2 text-[11px] leading-relaxed text-slate-400"
          >{{ agent.content }}</pre>
        </div>
      </div>
    </details>
  </div>
</template>

<style scoped>
details > summary::-webkit-details-marker {
  display: none;
}
details > summary::before {
  content: '▸';
  @apply inline-block w-4 shrink-0 text-slate-500 transition-transform align-top;
}
details[open] > summary::before {
  transform: rotate(90deg);
}
</style>
