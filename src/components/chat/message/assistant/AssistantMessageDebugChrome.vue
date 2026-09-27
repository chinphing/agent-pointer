<script setup lang="ts">
import { computed, toRef, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Code, Camera } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import { useAssistantMessageDebug } from '../../../../composables/useAssistantMessageDebug'
import MessageFooterActions from '../MessageFooterActions.vue'
import RawWirePanel from './RawWirePanel.vue'
import ScreenPreviewModal from './ScreenPreviewModal.vue'


const { t } = useI18n()
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
        :title="t('chat.message.viewInjectedScreenshot')"
        @click="openScreenPreview()"
      >
        <Camera class="w-3.5 h-3.5" />
      </button>
      <button
        v-if="showRawWireFooter"
        type="button"
        class="message-action-btn"
        :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
        :title="showRawWire ? t('chat.message.hideRaw') : t('chat.message.showRaw')"
        @click="showRawWire = !showRawWire"
      >
        <Code class="w-3.5 h-3.5" />
      </button>
    </template>
  </MessageFooterActions>

  <!-- Debug actions fallback when footer hidden (e.g. streaming without screenshot path yet) -->
  <div
    v-if="!showFooter && (showCamera || showRawWireFooter)"
    class="message-footer-actions flex items-center gap-1.5 px-3"
  >
    <button
      v-if="showCamera"
      type="button"
      class="message-action-btn text-muted hover:text-info disabled:opacity-40 disabled:cursor-wait"
      :disabled="screenLoading"
      :title="t('chat.message.viewInjectedScreenshot')"
      @click="openScreenPreview()"
    >
      <Camera class="w-3.5 h-3.5" />
    </button>
    <button
      v-if="showRawWireFooter"
      type="button"
      class="message-action-btn"
      :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
      :title="showRawWire ? t('chat.message.hideRaw') : t('chat.message.showRaw')"
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
