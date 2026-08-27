<script setup lang="ts">
import { computed } from 'vue'
import { toolCallKindIcon } from '../../lib/toolCallKindIcon'

defineOptions({ inheritAttrs: false })

const props = defineProps<{
  name: string
}>()

const icon = computed(() => toolCallKindIcon(props.name))
</script>

<template>
  <component
    :is="icon"
    v-bind="$attrs"
    class="h-3.5 w-3.5 shrink-0 text-muted/70"
    aria-hidden="true"
  />
</template>
