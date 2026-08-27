<script setup lang="ts">
import { computed, inject, ref, type Ref } from 'vue'
import type { ToolCall } from '../../types/chat'
import {
  collapsedLiveRunItemKey,
  collapsedToolListItems,
  shouldPinSubAgentHostRow,
  type ToolCallListItem
} from '../../lib/toolCallDisplay'
import ToolCallGroup from './ToolCallGroup.vue'
import ToolCallRow from './ToolCallRow.vue'

const props = defineProps<{
  toolCalls: ToolCall[]
  showToolCallResults?: boolean
  runActive?: boolean
  thinkingLine?: string | null
}>()

const searchToolCallIds = inject<Ref<string[]>>(
  'currentConversationSearchToolCallIds',
  ref<string[]>([])
)
const activeSearchToolCallId = inject<Ref<string | null>>(
  'currentConversationActiveToolCallId',
  ref<string | null>(null)
)

const items = computed(() =>
  collapsedToolListItems(props.toolCalls, {
    holdLiveSlot: props.runActive,
    thinkingLine: props.thinkingLine
  })
)

function isSearchMatch(toolCallId: string): boolean {
  return searchToolCallIds.value.includes(toolCallId)
}

function isActiveSearchMatch(toolCallId: string): boolean {
  return activeSearchToolCallId.value === toolCallId
}

function groupTools(item: Extract<ToolCallListItem, { kind: 'group' }>): ToolCall[] {
  return item.live ? [...item.tools, item.live] : item.tools
}

function groupIsSearchMatch(item: Extract<ToolCallListItem, { kind: 'group' }>): boolean {
  return groupTools(item).some(tc => isSearchMatch(tc.id))
}

function groupIsActiveSearchMatch(item: Extract<ToolCallListItem, { kind: 'group' }>): boolean {
  return groupTools(item).some(tc => isActiveSearchMatch(tc.id))
}

function itemKey(item: ToolCallListItem, index: number): string {
  return collapsedLiveRunItemKey(item, index, items.value.length, props.runActive === true)
}
</script>

<template>
  <div class="tool-call-list w-full space-y-0.5">
    <template
      v-for="(item, index) in items"
      :key="itemKey(item, index)"
    >
      <ToolCallGroup
        v-if="item.kind === 'group'"
        :tools="item.tools"
        :live-tool="item.live"
        :force-live-slot="runActive && index === items.length - 1"
        :thinking-line="runActive && index === items.length - 1 ? thinkingLine : null"
        :show-tool-call-results="showToolCallResults"
        :is-search-match="groupIsSearchMatch(item)"
        :is-active-search-match="groupIsActiveSearchMatch(item)"
      >
        <template #after-tool="slotProps">
          <slot
            name="after-tool"
            v-bind="slotProps"
          />
        </template>
      </ToolCallGroup>
      <template v-else>
        <div
          class="tool-call-item min-w-0 w-full"
          :data-keep-host="shouldPinSubAgentHostRow(item.tool) ? 'true' : undefined"
        >
          <ToolCallRow
            :tool-call="item.tool"
            :show-tool-call-results="showToolCallResults"
            :is-search-match="isSearchMatch(item.tool.id)"
            :is-active-search-match="isActiveSearchMatch(item.tool.id)"
          />
          <slot
            name="after-tool"
            :tool-call="item.tool"
          />
        </div>
      </template>
    </template>
  </div>
</template>
