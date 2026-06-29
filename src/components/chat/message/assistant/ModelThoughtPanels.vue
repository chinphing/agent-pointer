<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { SupervisorPlanTask } from '../../../../types/chat'

const props = defineProps<{
  xmlThoughts?: string
  planTasks?: SupervisorPlanTask[]
  thoughtsDebugEnabled?: boolean
  isStreaming?: boolean
}>()

const thoughtsBoxRef = ref<HTMLElement | null>(null)

const showXmlThoughts = computed(() => {
  const text = props.xmlThoughts?.trim()
  if (!text) return false
  if (props.isStreaming) return true
  return props.thoughtsDebugEnabled === true
})

const thoughtsUnlimitedHeight = computed(
  () => props.thoughtsDebugEnabled === true || !props.isStreaming
)

const showPlan = computed(() => (props.planTasks?.length ?? 0) > 0)

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

const hasContent = computed(() => showXmlThoughts.value || showPlan.value)
</script>

<template>
  <div v-if="hasContent" class="space-y-2">
    <div
      v-if="showXmlThoughts"
      ref="thoughtsBoxRef"
      class="text-[12px] leading-relaxed text-muted whitespace-pre-wrap rounded-md border border-border/60 bg-[hsl(var(--card-elevated))]/60 px-2.5 py-2"
      :class="thoughtsUnlimitedHeight ? '' : 'max-h-[3rem] overflow-y-auto overflow-x-hidden'"
    >{{ xmlThoughts }}</div>
    <ul
      v-if="showPlan"
      class="text-[12px] text-muted space-y-1 rounded-md border border-border/60 bg-[hsl(var(--card-elevated))]/40 px-2.5 py-2 list-none"
    >
      <li v-for="task in planTasks" :key="task.id" class="truncate">
        <span class="text-accent/80">{{ task.id }}</span>
        <span class="mx-1">·</span>
        {{ task.title }}
      </li>
    </ul>
  </div>
</template>
