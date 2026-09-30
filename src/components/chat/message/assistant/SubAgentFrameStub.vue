<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

import type { AgentTrace } from '../../../../types/chat'
import CollapsedRunHeader from '../../CollapsedRunHeader.vue'

defineProps<{
  trace: AgentTrace
  summaryLine: string
  /** One-line latest-round content preview (design doc §6.3, D-E1). */
  previewLine?: string
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
        :aria-label="summaryLine.trim() || t('chat.subtaskProcess')"
        @toggle="emit('toggle')"
      />
      <div
        v-if="previewLine"
        class="min-w-0 w-full pl-4 pr-2"
        data-sub-agent-content-preview
      >
        <span class="block truncate text-[12px] leading-5 text-muted/70">{{ previewLine }}</span>
      </div>
    </div>
  </div>
</template>
