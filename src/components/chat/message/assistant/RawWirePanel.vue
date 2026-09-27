<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { computed, ref } from 'vue'
import { Check, Copy } from 'lucide-vue-next'

const props = defineProps<{
  /** API `reasoning_content` 流（与正文分开通道）；仅在此面板内展示，不写入主气泡。 */
  reasoning?: string
  /** 模型正文通道的未裁剪原始输出（`content` delta）。 */
  rawContent?: string
  /** 工具调用参数（按调用顺序拼接，不含结果）。 */
  toolRawArgs?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const copied = ref(false)

const reasoningText = computed(() => props.reasoning?.trim() ?? '')
const outputText = computed(() => props.rawContent ?? '')
const toolRawArgsText = computed(() => props.toolRawArgs?.trim() ?? '')

/** 代码图标打开的「原始输出」：推理与正文通道原始字符合并为一处，便于复制与对照。 */
const combinedRawText = computed(() => {
  const r = reasoningText.value
  const o = outputText.value
  const parts: string[] = []
  if (r.length > 0) parts.push(`${t('chat.rawReasoningHeader')}\n${r}`)
  if (o.length > 0) parts.push(`${t('chat.rawContentHeader')}\n${o}`)
  if (toolRawArgsText.value.length > 0) {
    parts.push(`${t('chat.rawToolArgsHeader')}\n${toolRawArgsText.value}`)
  }
  return parts.join('\n\n')
})

const hasAnything = computed(() => combinedRawText.value.length > 0)

function copyAll() {
  if (!combinedRawText.value) return
  void navigator.clipboard.writeText(combinedRawText.value).then(() => {
    copied.value = true
    window.setTimeout(() => {
      copied.value = false
    }, 2000)
  })
}
</script>

<template>
  <div class="raw-wire-panel mt-2 w-full overflow-hidden">
    <div class="raw-wire-header">
      <span class="text-[11px] font-medium text-foreground">{{ t('chat.s_13cbd4') }}</span>
      <div class="flex items-center gap-1">
        <button
          v-if="hasAnything"
          type="button"
          class="raw-wire-action"
          :title="copied ? t('common.copied') : t('chat.copyAll')"
          @click="copyAll"
        >
          <Check v-if="copied" class="w-3 h-3 text-success" />
          <Copy v-else class="w-3 h-3" />
        </button>
        <button type="button" class="raw-wire-action text-[11px]" @click="emit('close')">
          {{ t('shell.collapse') }}
        </button>
      </div>
    </div>

    <div class="max-h-96 overflow-auto p-3 space-y-3">
      <section v-if="reasoningText">
        <div class="raw-wire-section-label">{{ t('chat.s_ed6f7e') }}</div>
        <pre class="raw-wire-block">{{ reasoningText }}</pre>
      </section>

      <section v-if="outputText">
        <div class="raw-wire-section-label">{{ t('chat.s_ea3cec') }}</div>
        <pre class="raw-wire-block">{{ outputText }}</pre>
      </section>

      <section v-if="toolRawArgsText">
        <div class="raw-wire-section-label">{{ t('chat.s_827c50') }}</div>
        <pre class="raw-wire-block">{{ toolRawArgsText }}</pre>
      </section>

      <div v-if="!hasAnything" class="text-[11px] text-muted py-1">
        {{ t('chat.s_4726ff') }}
      </div>
    </div>
  </div>
</template>
