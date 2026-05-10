<script setup lang="ts">
defineProps<{
  rawContent?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()
</script>

<template>
  <div class="mt-2 w-full rounded-xl border border-primary/20 bg-black/30 overflow-hidden">
    <div class="flex items-center justify-between px-3 py-1.5 border-b border-primary/10 bg-primary/5">
      <span class="text-[11px] font-medium text-primary-cyan">原始输出</span>
      <button
        type="button"
        class="text-[11px] text-slate-400 hover:text-slate-200 transition"
        @click="emit('close')"
      >
        收起
      </button>
    </div>
    <pre
      class="max-h-80 overflow-auto p-3 text-[11px] leading-relaxed text-slate-300 whitespace-pre-wrap"
    >{{ rawContent }}</pre>
  </div>
</template>
