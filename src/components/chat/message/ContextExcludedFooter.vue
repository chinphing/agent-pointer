<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage } from '../../../types/chat'
import { excludedReasonLabel, isContextExcluded } from '../../../lib/messageContext'

const props = defineProps<{ message: ChatMessage }>()

const show = computed(() => isContextExcluded(props.message))

const tooltip = computed(() =>
  excludedReasonLabel(props.message.contextState?.excludedReason)
)
</script>

<template>
  <div
    v-if="show"
    class="mt-1.5 pt-1 border-t border-border/60 text-[10px] text-muted leading-none"
    :title="tooltip"
  >
    未纳入上下文
  </div>
</template>
