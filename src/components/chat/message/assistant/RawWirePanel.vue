<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  /** API `reasoning_content` 流（与正文分开通道）。 */
  reasoning?: string
  /** 模型正文通道的未裁剪原始输出（`content` delta）。 */
  rawContent?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const reasoningText = computed(() => props.reasoning?.trim() ?? '')
const outputText = computed(() => props.rawContent ?? '')

const hasReasoning = computed(() => reasoningText.value.length > 0)
const hasOutput = computed(() => outputText.value.length > 0)
</script>

<template>
  <div class="mt-2 w-full rounded-xl border border-primary/20 bg-black/30 overflow-hidden">
    <div class="flex items-center justify-between px-3 py-1.5 border-b border-primary/10 bg-primary/5">
      <span class="text-[11px] font-medium text-primary-cyan">推理与原始输出</span>
      <button
        type="button"
        class="text-[11px] text-slate-400 hover:text-slate-200 transition"
        @click="emit('close')"
      >
        收起
      </button>
    </div>

    <div class="max-h-96 overflow-auto divide-y divide-primary/10">
      <section v-if="hasReasoning" class="p-3">
        <div class="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-amber-200/85">
          模型推理
        </div>
        <pre
          class="text-[11px] leading-relaxed text-slate-300 whitespace-pre-wrap break-words"
        >{{ reasoningText }}</pre>
      </section>

      <section v-if="hasOutput" class="p-3">
        <div class="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-cyan-200/80">
          模型输出
        </div>
        <pre
          class="text-[11px] leading-relaxed text-slate-300 whitespace-pre-wrap break-words"
        >{{ outputText }}</pre>
      </section>

      <div
        v-if="!hasReasoning && !hasOutput"
        class="p-3 text-[11px] text-slate-500"
      >
        暂无内容
      </div>
    </div>
  </div>
</template>
