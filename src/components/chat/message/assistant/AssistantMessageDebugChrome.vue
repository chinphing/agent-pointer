<script setup lang="ts">
import { computed, toRef, type Ref } from 'vue'
import { Code, Camera } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import { useAssistantMessageDebug } from '../../../../composables/useAssistantMessageDebug'
import MessageFooterActions from '../MessageFooterActions.vue'
import RawWirePanel from './RawWirePanel.vue'
import ScreenPreviewModal from './ScreenPreviewModal.vue'

const props = defineProps<{
  message: ChatMessage
  copyText?: string
  showCopy?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
}>()

const messageRef = toRef(props, 'message') as Ref<ChatMessage>

const {
  showRawWire,
  modalOpen,
  screenLoading,
  screenPreview,
  screenError,
  rawWireReasoning,
  rawWireContent,
  toolRawArgs,
  hasRawWire,
  showCamera,
  showMessageActions,
  showDebugActions,
  showRawWireFooter,
  openScreenPreview
} = useAssistantMessageDebug(messageRef, {
  generating: toRef(props, 'generating'),
  isActiveGenerationMessage: toRef(props, 'isActiveGenerationMessage')
})

const footerCopyText = computed(() => props.copyText?.trim() ?? '')

const showCopyButton = computed(() => {
  if (props.showCopy === false) return false
  return props.showCopy === true && !!footerCopyText.value
})

const showFooter = computed(() => showMessageActions.value)
</script>

<template>
  <MessageFooterActions
    v-if="showFooter"
    class="px-3 !mt-0"
    :created-at="message.createdAt"
    :copy-text="footerCopyText"
    :show-copy="showCopyButton"
  >
    <template v-if="showDebugActions || showRawWireFooter" #extra>
      <button
        v-if="showCamera"
        type="button"
        class="message-action-btn text-muted hover:text-info disabled:opacity-40 disabled:cursor-wait"
        :disabled="screenLoading"
        title="查看本轮已注入模型的桌面截图（缓存）"
        @click="openScreenPreview()"
      >
        <Camera class="w-3.5 h-3.5" />
      </button>
      <button
        v-if="showRawWireFooter"
        type="button"
        class="message-action-btn"
        :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
        :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
        @click="showRawWire = !showRawWire"
      >
        <Code class="w-3.5 h-3.5" />
      </button>
    </template>
  </MessageFooterActions>

  <!-- Raw content toggle: fallback when footer hidden (e.g. streaming), still hover-dependent -->
  <div v-if="!showFooter && showRawWireFooter" class="message-footer-actions flex items-center gap-1.5 px-3">
    <button
      type="button"
      class="message-action-btn"
      :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
      :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
      @click="showRawWire = !showRawWire"
    >
      <Code class="w-3.5 h-3.5" />
    </button>
  </div>

  <RawWirePanel
    v-if="showRawWire && (rawWireContent || rawWireReasoning || toolRawArgs)"
    :reasoning="rawWireReasoning"
    :raw-content="rawWireContent"
    :tool-raw-args="toolRawArgs"
    @close="showRawWire = false"
  />

  <ScreenPreviewModal
    v-model:open="modalOpen"
    :loading="screenLoading"
    :preview="screenPreview"
    :error="screenError"
  />
</template>
