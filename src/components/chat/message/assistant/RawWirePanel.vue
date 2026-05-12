<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  /** API `reasoning_content` 流（与正文分开通道）；仅在此面板内展示，不写入主气泡。 */
  reasoning?: string
  /** 模型正文通道的未裁剪原始输出（`content` delta）。 */
  rawContent?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const reasoningText = computed(() => props.reasoning?.trim() ?? '')
const outputText = computed(() => props.rawContent ?? '')

/** 代码图标打开的「原始输出」：推理与正文通道原始字符合并为一处，便于复制与对照。 */
const combinedRawText = computed(() => {
  const r = reasoningText.value
  const o = outputText.value
  const parts: string[] = []
  if (r.length > 0) parts.push(`【推理】\n${r}`)
  if (o.length > 0) parts.push(`【正文通道原始】\n${o}`)
  return parts.join('\n\n')
})

const hasAnything = computed(() => combinedRawText.value.length > 0)
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

    <div class="max-h-96 overflow-auto p-3">
      <pre
        v-if="hasAnything"
        class="text-[11px] leading-relaxed text-slate-300 whitespace-pre-wrap break-words"
      >{{ combinedRawText }}</pre>
      <div v-else class="text-[11px] text-slate-500">
        暂无内容
      </div>
    </div>
  </div>
</template>
