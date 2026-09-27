<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ShieldCheck, X } from 'lucide-vue-next'
import { approveChannelPairingAny, type PairingPendingItem } from '../../lib/channels'
import { channelLabel } from '../../lib/channel-labels'
import { useChatStore } from '../../stores/chat'

const { t } = useI18n()
const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  pending: PairingPendingItem | null
}>()

const emit = defineEmits<{
  (e: 'approved'): void
  (e: 'dismiss'): void
}>()

const chat = useChatStore()
const pairingCode = ref('')
const error = ref('')
const busy = ref(false)

watch(
  () => props.pending,
  item => {
    pairingCode.value = item?.code?.trim() ?? ''
    error.value = ''
  },
  { immediate: true }
)

watch(open, v => {
  if (!v) error.value = ''
})

function close() {
  open.value = false
  emit('dismiss')
}

async function approve() {
  const code = pairingCode.value.trim()
  if (!code) return
  busy.value = true
  error.value = ''
  try {
    const channel = await approveChannelPairingAny('default', code)
    chat.showUiToast(t('channelPairing.successToast', { channel: channelLabel(channel) }), 'success')
    emit('approved')
    open.value = false
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && !busy.value) close()
  if (e.key === 'Enter' && !busy.value) void approve()
}

watch(
  open,
  v => {
    if (v) document.addEventListener('keydown', onKeydown)
    else document.removeEventListener('keydown', onKeydown)
  },
  { immediate: true }
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[220] flex items-center justify-center bg-foreground/32 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      :aria-label="t('channelPairing.ariaLabel')"
      @click.self="!busy && close()"
    >
      <div
        class="relative w-full max-w-md overflow-hidden rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
      >
        <header class="flex items-start gap-3 border-b border-border px-5 pb-3 pt-5">
          <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent/15">
            <ShieldCheck class="h-4 w-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">{{ t('channelPairing.title') }}</h2>
            <p class="mt-1 text-[13px] leading-relaxed text-muted">
              <template v-if="pending">
                <span class="text-foreground">{{ channelLabel(pending.channel) }}</span>
                {{ t('channelPairing.descriptionWithPending') }}
              </template>
              <template v-else>
                {{ t('channelPairing.descriptionEmpty') }}
              </template>
            </p>
          </div>
          <button
            type="button"
            class="absolute right-4 top-4 cursor-pointer rounded-lg p-2 text-muted transition-colors hover:bg-hover"
            :title="t('channelPairing.closeLaterTitle')"
            :disabled="busy"
            @click="close"
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <div class="space-y-3 px-5 py-4">
          <div>
            <label class="mb-1.5 block text-xs text-muted">{{ t('channelPairing.pairingCodeLabel') }}</label>
            <input
              v-model="pairingCode"
              type="text"
              class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm font-mono tracking-wider placeholder:text-muted"
              :placeholder="t('channelPairing.pairingCodePlaceholder')"
              autocomplete="off"
              :disabled="busy"
            />
          </div>
          <p v-if="error" class="text-xs text-danger">{{ error }}</p>
        </div>

        <footer class="flex justify-end gap-2 border-t border-border px-5 py-4">
          <button
            type="button"
            class="h-9 cursor-pointer rounded-lg bg-hover px-4 text-sm text-foreground transition-opacity hover:opacity-90"
            :disabled="busy"
            @click="close"
          >
            {{ t('channelPairing.later') }}
          </button>
          <button
            type="button"
            class="h-9 cursor-pointer rounded-lg bg-accent px-4 text-sm font-medium text-accent-foreground transition-opacity hover:opacity-95 disabled:opacity-50"
            :disabled="busy || !pairingCode.trim()"
            @click="approve"
          >
            {{ busy ? t('channelPairing.approving') : t('channelPairing.approve') }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
