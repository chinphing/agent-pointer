<script setup lang="ts">
const props = defineProps<{
  text: string
  align?: 'start' | 'end'
}>()
</script>

<template>
  <p
    class="text-[11px] leading-relaxed text-muted-foreground/75 max-w-sm break-words"
    :class="props.align === 'end' ? 'text-right' : 'text-left'"
  >
    {{ text }}
  </p>
</template>
