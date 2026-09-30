<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChatStore } from '../../stores/chat'
import { useConversationScopedStore } from '../../lib/conversationScoped'
import {
  ASK_USER_BANNER_LINGER_MS,
  createAskUserLinger,
  pendingAskUserToolCalls,
  resolveAskUserBannerView,
  type AskUserBannerLinger
} from '../../lib/askUserBanner'
import AskUserOptions from './AskUserOptions.vue'

/**
 * Top-of-chat pending `ask_user` surface (design doc §4).
 *
 * Sources the queue from the lead messages **and** the conversation's scoped
 * rows, so a deep sub-agent question shows even when no frame is mounted.
 */
const { t } = useI18n()
const chat = useChatStore()
const scopedStore = useConversationScopedStore()

const linger = ref<AskUserBannerLinger | null>(null)
let lingerTimer: ReturnType<typeof setTimeout> | null = null

const pending = computed(() => {
  const convId = (chat.currentId ?? chat.current?.id ?? '').trim()
  const scopedRows = convId ? scopedStore.listRows(convId) : []
  if (convId) {
    // Scoped rows live behind a shallowRef map: spawn membership + the ask_user
    // revision are the reactive hooks that make new rows / status changes re-evaluate
    // this computed. The live fingerprint is deliberately NOT read — it is replaced on
    // every streamed chunk, which used to rescan every row per token.
    void scopedStore.getMembershipSignal(convId)
    void scopedStore.getAskUserRevision(convId)
  }
  return pendingAskUserToolCalls([...(chat.current?.messages ?? []), ...scopedRows])
})

const view = computed(() => resolveAskUserBannerView(pending.value, linger.value, Date.now()))

const lingerText = computed(() =>
  view.value?.kind === 'linger'
    ? t('chat.selectedList', { items: view.value.selected.join(t('common.listSep')) })
    : ''
)

function clearLingerTimer() {
  if (lingerTimer == null) return
  clearTimeout(lingerTimer)
  lingerTimer = null
}

function startLinger(toolCallId: string, selected: string[], knownPendingIds: string[]) {
  clearLingerTimer()
  linger.value = createAskUserLinger({
    toolCallId,
    selected,
    knownPendingIds,
    now: Date.now(),
    lingerMs: ASK_USER_BANNER_LINGER_MS
  })
  lingerTimer = setTimeout(() => {
    lingerTimer = null
    linger.value = null
  }, ASK_USER_BANNER_LINGER_MS)
}

/** AskUserOptions emits after a successful submit — the 2s starts here (D-C3). */
function onSubmitted(labels: string[]) {
  const current = view.value
  if (!current || current.kind !== 'ask') return
  startLinger(
    current.toolCall.id,
    labels,
    pending.value.map(tc => tc.id).filter(Boolean)
  )
}

watch(
  () => chat.currentId,
  () => {
    clearLingerTimer()
    linger.value = null
  }
)

onBeforeUnmount(clearLingerTimer)
</script>

<template>
  <div
    v-if="view"
    class="shrink-0 px-3 pb-1 pt-2"
    data-ask-user-banner
  >
    <div class="chat-column mx-auto w-full">
      <div class="rounded-lg border border-border bg-background/95 shadow-sm backdrop-blur">
        <div
          v-if="view.remaining > 0"
          class="border-b border-border px-3 py-1 text-[11px] text-muted"
          data-ask-user-banner-remaining
        >
          {{ t('chat.askUserBannerRemaining', { count: view.remaining }) }}
        </div>
        <div class="px-3 py-2">
          <p
            v-if="view.kind === 'linger'"
            class="text-xs text-muted"
            data-ask-user-banner-linger
          >
            {{ lingerText }}
          </p>
          <AskUserOptions
            v-else
            :tool-call="view.toolCall"
            @submitted="onSubmitted"
          />
        </div>
      </div>
    </div>
  </div>
</template>
