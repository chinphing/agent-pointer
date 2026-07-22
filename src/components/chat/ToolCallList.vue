<script setup lang="ts">
import { inject, ref, type Ref } from 'vue'
import type { ToolCall } from '../../types/chat'
import ToolCallRow from './ToolCallRow.vue'

const props = defineProps<{
  toolCalls: ToolCall[]
  showToolCallResults?: boolean
}>()

const searchToolCallIds = inject<Ref<string[]>>(
  'currentConversationSearchToolCallIds',
  ref<string[]>([])
)
const activeSearchToolCallId = inject<Ref<string | null>>(
  'currentConversationActiveToolCallId',
  ref<string | null>(null)
)

function isSearchMatch(toolCallId: string): boolean {
  return searchToolCallIds.value.includes(toolCallId)
}

function isActiveSearchMatch(toolCallId: string): boolean {
  return activeSearchToolCallId.value === toolCallId
}
</script>

<template>
  <div class="tool-call-list w-full space-y-0.5">
    <template
      v-for="tc in toolCalls"
      :key="tc.id"
    >
      <ToolCallRow
        :tool-call="tc"
        :show-tool-call-results="showToolCallResults"
        :is-search-match="isSearchMatch(tc.id)"
        :is-active-search-match="isActiveSearchMatch(tc.id)"
      />
      <slot
        name="after-tool"
        :tool-call="tc"
      />
    </template>
  </div>
</template>
