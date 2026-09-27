<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

import { ref } from 'vue'
import { Copy, Check } from 'lucide-vue-next'
import MessageTimeChip from './MessageTimeChip.vue'

const props = defineProps<{
  createdAt: number
  copyText?: string
  showCopy?: boolean
}>()

const copied = ref(false)

function copy() {
  const text = props.copyText?.trim() ?? ''
  if (!text) return
  void navigator.clipboard.writeText(text).then(() => {
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  })
}

const showCopyButton = () => props.showCopy === true && !!(props.copyText?.trim())
</script>

<template>
  <div class="message-footer-actions flex items-center gap-1.5">
    <MessageTimeChip :created-at="createdAt" />
    <button
      v-if="showCopyButton()"
      type="button"
      class="message-action-btn"
      :class="copied ? 'text-success' : 'text-muted hover:text-foreground'"
      :title="copied ? t('common.copied') : t('common.copy')"
      @click="copy"
    >
      <Check v-if="copied" class="w-3 h-3" />
      <Copy v-else class="w-3 h-3" />
    </button>
    <slot name="extra" />
  </div>
</template>
