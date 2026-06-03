<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage } from '../../../types/chat'
import { glueMessagePreview } from '../../../lib/threadLayoutGlue'

const props = defineProps<{ message: ChatMessage }>()

const preview = computed(() => glueMessagePreview(props.message))
</script>

<template>
  <div
    class="tool-run-glue-row px-3 py-0 text-[10px] leading-snug text-muted/75"
    :title="message.content?.trim() || undefined"
  >
    {{ preview }}
  </div>
</template>
