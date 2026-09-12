<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { ToolCall } from '../../types/chat'
import { useChatStore } from '../../stores/chat'
import {
  compactToolCallLiveText,
  compactToolCallStatusLine,
  formatCollapsedToolGroupLine
} from '../../lib/toolCallDisplay'
import CollapsedRunHeader from './CollapsedRunHeader.vue'
import ToolCallRow from './ToolCallRow.vue'

const props = defineProps<{
  tools: ToolCall[]
  liveTool?: ToolCall | null
  forceLiveSlot?: boolean
  thinkingLine?: string | null
  showToolCallResults?: boolean
  isSearchMatch?: boolean
  isActiveSearchMatch?: boolean
}>()

const chat = useChatStore()
const expanded = ref(false)

watch(
  () => props.isActiveSearchMatch || props.isSearchMatch,
  match => {
    if (match) expanded.value = true
  },
  { immediate: true }
)

const summaryLine = computed(() => {
  const groupLine = formatCollapsedToolGroupLine(props.tools, chat.current?.workspaceRoot)
  if (groupLine) return groupLine
  // Empty live group (first-round / post-ask_user gap): keep summary blank and
  // put「思考中」on the live slot so it matches mid-run tool gaps.
  if (props.tools.length === 0) return ''
  if (!props.liveTool && props.thinkingLine?.trim()) return props.thinkingLine.trim()
  return ''
})

const liveLine = computed(() => {
  if (props.liveTool) {
    return compactToolCallLiveText(props.liveTool, chat.current?.workspaceRoot)
  }
  return props.thinkingLine?.trim() || null
})

const liveToolName = computed(() => props.liveTool?.name ?? null)

const headerAriaLabel = computed(() => {
  if (summaryLine.value.trim()) return summaryLine.value
  if (props.liveTool) {
    return compactToolCallStatusLine(props.liveTool, chat.current?.workspaceRoot, {
      includeStatus: false
    })
  }
  return liveLine.value || '思考中'
})

const liveKey = computed(() => {
  if (props.liveTool) return props.liveTool.id
  if (props.thinkingLine?.trim()) return 'thinking'
  return null
})

const expandedTools = computed(() => {
  if (!props.liveTool) return props.tools
  if (props.tools.some(tc => tc.id === props.liveTool?.id)) return props.tools
  return [...props.tools, props.liveTool]
})

function toggleExpanded() {
  if (expandedTools.value.length === 0) return
  expanded.value = !expanded.value
}
</script>

<template>
  <div
    class="tool-call-group min-w-0 w-full"
    :class="isActiveSearchMatch
      ? 'rounded-lg ring-2 ring-accent/60 bg-accent/10'
      : isSearchMatch
        ? 'rounded-lg bg-accent/5'
        : ''"
  >
    <CollapsedRunHeader
      :summary-line="summaryLine"
      :live-line="expanded ? null : liveLine"
      :live-tool-name="expanded ? null : liveToolName"
      :live-key="expanded ? null : liveKey"
      :expanded="expanded"
      :force-live-slot="!expanded && forceLiveSlot"
      :show-chevron="expandedTools.length > 0"
      :live-busy="liveTool?.status === 'running'"
      :aria-label="headerAriaLabel"
      @toggle="toggleExpanded"
    />

    <div
      v-if="expanded"
      class="space-y-0.5"
    >
      <template
        v-for="tc in expandedTools"
        :key="tc.id"
      >
        <ToolCallRow
          :tool-call="tc"
          :show-tool-call-results="showToolCallResults"
        />
        <slot
          name="after-tool"
          :tool-call="tc"
        />
      </template>
    </div>
    <template v-else>
      <span
        v-for="tc in expandedTools"
        :key="`anchor-${tc.id}`"
        class="sr-only"
        :data-tool-call-id="tc.id"
      />
    </template>
  </div>
</template>
