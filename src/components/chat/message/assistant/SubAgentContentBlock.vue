<script setup lang="ts">
import { onMounted, onUpdated, ref } from 'vue'
import { bump, renderPerfEnabled } from '../../../../lib/renderPerf'

// Dev-only render perf counters — one boolean check, no-op unless the HUD is on.
onMounted(() => {
  if (!renderPerfEnabled()) return
  bump('mount:SubAgentContentBlock')
})
onUpdated(() => {
  if (!renderPerfEnabled()) return
  bump('render:SubAgentContentBlock')
})
import { parseMarkdown } from '../../../../lib/markdownConfig'
import { useThrottledMarkdown } from '../../../../composables/useThrottledMarkdown'
import { useMarkdownCodeCopy } from '../../../../composables/useMarkdownCodeCopy'
import { useMarkdownCharts } from '../../../../composables/useMarkdownCharts'
import { useMarkdownSvgs } from '../../../../composables/useMarkdownSvgs'
import { useMarkdownMermaid } from '../../../../composables/useMarkdownMermaid'
import { useMarkdownExternalLinks } from '../../../../composables/useMarkdownExternalLinks'

/**
 * One sub-agent round's assistant text (design doc §6.3).
 *
 * Deliberately **not** `AgentMessageBody`: that block carries lead-only chrome
 * (copy button, media gallery, task board, platform balance). This one keeps the
 * shared markdown pipeline — parse, streaming throttle, diagram renderers, code
 * copy — and nothing else.
 */
const props = defineProps<{
  content: string
  /** Scoped row id of the round; the conversation search targets this element. */
  messageId?: string
  streaming?: boolean
  isSearchMatch?: boolean
  isActiveSearchMatch?: boolean
}>()

const root = ref<HTMLElement | null>(null)

const html = useThrottledMarkdown(
  () => props.content,
  () => props.streaming === true,
  src =>
    parseMarkdown(src, {
      streamingCharts: props.streaming === true,
      streamingSvgs: props.streaming === true,
      streamingMermaid: props.streaming === true
    }),
  { longSourceThreshold: 8000, longStreamingInterval: 250 }
)

useMarkdownCodeCopy(root, () => html.value)
useMarkdownCharts(root, () => html.value, {
  isStreaming: () => props.streaming === true
})
useMarkdownSvgs(root, () => html.value, {
  isStreaming: () => props.streaming === true
})
useMarkdownMermaid(root, () => html.value, {
  isStreaming: () => props.streaming === true
})
useMarkdownExternalLinks(root, () => html.value)
</script>

<template>
  <div
    v-if="html"
    ref="root"
    class="sub-agent-content md-body md-body-flow px-3 min-w-0 w-full max-w-full break-words overflow-x-hidden"
    :data-sub-agent-content-id="messageId"
    :class="isActiveSearchMatch
      ? 'rounded-lg ring-2 ring-accent/60 bg-accent/10'
      : isSearchMatch
        ? 'rounded-lg bg-accent/5'
        : ''"
    v-html="html"
  />
</template>
