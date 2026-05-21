<script setup lang="ts">
import { computed } from 'vue'
import type { AgentTrace, SupervisorPlanTask } from '../../../../types/chat'
import AgentProgressTimeline from './AgentProgressTimeline.vue'

const props = defineProps<{
  xmlThoughts?: string
  agentTrace?: AgentTrace[]
  planTasks?: SupervisorPlanTask[]
  showThoughts?: boolean
  showSubAgentTrace: boolean
  isStreaming?: boolean
}>()

const showXmlThoughts = computed(
  () => props.showThoughts !== false && !!props.xmlThoughts?.trim()
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
      class="text-[12px] leading-relaxed text-muted whitespace-pre-wrap"
    >{{ xmlThoughts }}</div>
    <AgentProgressTimeline
      v-if="showTracePanel"
      :agent-trace="agentTrace"
      :plan-tasks="planTasks"
      :is-streaming="isStreaming"
    />
  </div>
</template>
