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
 * Pending `ask_user` surface for **sub-agents**, mounted by `Composer` above the input
 * box — the same auxiliary stack as the background-job / outbound-queue panels
 * (design doc §4).
 *
 * The lead agent's own `ask_user` needs no banner: its card renders inline in the
 * transcript. Only scoped rows (any spawn depth) are queued here, so a sub-agent
 * question shows even when its frame is collapsed or not mounted.
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
  return pendingAskUserToolCalls(scopedRows)
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
    data-ask-user-banner
  >
    <div
      v-if="view.remaining > 0"
      class="mb-1 px-1 text-[11px] text-muted"
      data-ask-user-banner-remaining
    >
      {{ t('chat.askUserBannerRemaining', { count: view.remaining }) }}
    </div>
    <p
      v-if="view.kind === 'linger'"
      class="mb-2 px-1 text-xs text-muted"
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
</template>
