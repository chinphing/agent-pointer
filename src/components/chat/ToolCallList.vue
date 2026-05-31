<script setup lang="ts">
import type { ToolCall } from '../../types/chat'
import ToolCallRow from './ToolCallRow.vue'

defineProps<{
  toolCalls: ToolCall[]
  showToolCallResults?: boolean
}>()
</script>

<template>
  <div class="tool-call-list w-full space-y-0.5">
    <ToolCallRow
      v-for="tc in toolCalls"
      :key="tc.id"
      :tool-call="tc"
      :show-tool-call-results="showToolCallResults"
    />
  </div>
</template>
