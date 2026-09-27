<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { AgentTrace } from '../../../../types/chat'
import CollapsedRunHeader from '../../CollapsedRunHeader.vue'


const { t } = useI18n()
defineProps<{
  trace: AgentTrace
  summaryLine: string
  showChevron: boolean
}>()

const emit = defineEmits<{
  toggle: []
}>()
</script>

<template>
  <div
    class="sub-agent-frame-stub min-w-0 w-full overflow-hidden"
    :style="{
      marginLeft: `${Math.max(0, (trace.depth ?? 1) - 1) * 12}px`
    }"
  >
    <div class="sub-agent-nested min-w-0 w-full">
      <CollapsedRunHeader
        :summary-line="summaryLine"
        :expanded="false"
        :failed="trace.status === 'failed'"
        :show-chevron="showChevron"
        :aria-label="summaryLine.trim() || t('chat.message.subtaskProcess')"
        @toggle="emit('toggle')"
      />
    </div>
  </div>
</template>
