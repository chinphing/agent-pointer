<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { AgentTrace, SupervisorPlanTask } from '../../../../types/chat'
import AgentProgressTimeline from './AgentProgressTimeline.vue'

const props = defineProps<{
  xmlThoughts?: string
  agentTrace?: AgentTrace[]
  planTasks?: SupervisorPlanTask[]
  /** Debug 模式且开启「显示 thoughts 摘要」时为 true，thoughts 不限高度并在完成后保留。 */
  thoughtsDebugEnabled?: boolean
  showSubAgentTrace: boolean
  isStreaming?: boolean
}>()

const thoughtsBoxRef = ref<HTMLElement | null>(null)

const showXmlThoughts = computed(() => {
  const text = props.xmlThoughts?.trim()
  if (!text) return false
  if (props.isStreaming) return true
  return props.thoughtsDebugEnabled === true
})

const thoughtsUnlimitedHeight = computed(() => props.thoughtsDebugEnabled === true)

watch(
  () => props.xmlThoughts,
  () => {
    if (thoughtsUnlimitedHeight.value || !props.isStreaming) return
    const el = thoughtsBoxRef.value
    if (!el) return
    requestAnimationFrame(() => {
      el.scrollTop = el.scrollHeight
    })
  }
)

const showTracePanel = computed(
  () =>
    props.showSubAgentTrace &&
    ((props.agentTrace?.length ?? 0) > 0 || (props.planTasks?.length ?? 0) > 0)
)

const hasContent = computed(() => showXmlThoughts.value || showTracePanel.value)
</script>

<template>
  <div v-if="hasContent" class="space-y-2">
    <div
      v-if="showXmlThoughts"
      ref="thoughtsBoxRef"
      class="text-[12px] leading-relaxed text-muted whitespace-pre-wrap rounded-md border border-border/60 bg-[hsl(var(--card-elevated))]/60 px-2.5 py-2"
      :class="thoughtsUnlimitedHeight ? '' : 'max-h-[3rem] overflow-y-auto overflow-x-hidden'"
    >{{ xmlThoughts }}</div>
    <AgentProgressTimeline
      v-if="showTracePanel"
      :agent-trace="agentTrace"
      :plan-tasks="planTasks"
      :is-streaming="isStreaming"
    />
  </div>
</template>
