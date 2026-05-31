<script setup lang="ts">
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
      :title="copied ? '已复制' : '复制'"
      @click="copy"
    >
      <Check v-if="copied" class="w-3.5 h-3.5" />
      <Copy v-else class="w-3.5 h-3.5" />
    </button>
    <slot name="extra" />
  </div>
</template>
