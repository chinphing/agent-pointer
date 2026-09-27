<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { AlertCircle } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import MessageFooterActions from '../MessageFooterActions.vue'
import { isBalanceExhaustedMessage, openPlatformBillingPage } from '../../../../lib/platformUrls'
import { usePlatformAuthStore } from '../../../../stores/platformAuth'

const { t } = useI18n()


const props = defineProps<{ message: ChatMessage }>()
const platformAuth = usePlatformAuthStore()

const showRecharge = computed(
  () =>
    !platformAuth.isStandalone &&
    isBalanceExhaustedMessage(props.message.errorMessage)
)

async function onOpenBilling() {
  try {
    await openPlatformBillingPage()
  } catch (e) {
    console.warn('[chat] open billing page failed', e)
  }
}
</script>

<template>
  <div
    class="message-stamp-host w-full rounded-2xl border border-danger/40 bg-danger/10 px-3 text-[14px] leading-relaxed text-foreground"
    role="alert"
  >
    <div class="flex gap-2 items-start min-w-0">
      <AlertCircle class="w-4 h-4 shrink-0 mt-0.5 text-danger" />
      <div class="min-w-0 flex-1">
        <div class="text-[11px] uppercase tracking-wide text-danger/90 mb-1">{{ t('chat.assistantError') }}</div>
        <div class="whitespace-pre-wrap break-words">
          {{
            showRecharge
              ? t('chat.balanceExhaustedContinue')
              : message.errorMessage || t('chat.generateFailed')
          }}
        </div>
        <button
          v-if="showRecharge"
          type="button"
          class="mt-2 inline-flex rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer"
          @click="onOpenBilling"
        >
          {{ t('chat.goRecharge') }}
        </button>
      </div>
    </div>
    <MessageFooterActions
      :created-at="message.createdAt"
      :copy-text="message.errorMessage || t('chat.generateFailed')"
      :show-copy="true"
    />
  </div>
</template>
