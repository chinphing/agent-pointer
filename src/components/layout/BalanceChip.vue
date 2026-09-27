<script setup lang="ts">
import { AlertTriangle, WalletCards } from 'lucide-vue-next'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { usePlatformBalance } from '../../composables/usePlatformBalance'
import { openPlatformBillingPage } from '../../lib/platformUrls'

const { t } = useI18n()
const { balance, exhausted, loading, lowBalance, visible } = usePlatformBalance()

const toneClass = computed(() => {
  if (exhausted.value) return 'border-warning/40 bg-warning/10 text-warning'
  if (lowBalance.value) return 'border-warning/25 bg-warning/5 text-warning'
  return 'border-border bg-card text-foreground'
})

const balanceLabel = computed(() => {
  if (loading.value && balance.value == null) return t('shell.balanceLoading')
  return balance.value == null
    ? t('shell.balanceUnknown')
    : t('shell.balance', { amount: balance.value })
})
</script>

<template>
  <div
    v-if="visible"
    class="inline-flex h-7 shrink-0 items-center gap-1 rounded-full border pl-2 text-xs shadow-sm"
    :class="toneClass"
    :title="exhausted ? t('shell.balanceExhausted') : lowBalance ? t('shell.balanceLow') : undefined"
  >
    <AlertTriangle v-if="exhausted || lowBalance" class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
    <WalletCards v-else class="h-3.5 w-3.5 shrink-0 text-muted" aria-hidden="true" />
    <span class="tabular-nums whitespace-nowrap">{{ balanceLabel }}</span>
    <button
      type="button"
      class="h-6 rounded-full border border-border/80 bg-background px-2 text-[11px] font-medium text-foreground hover:bg-hover cursor-pointer"
      :aria-label="t('shell.goRecharge')"
      @click="openPlatformBillingPage"
    >
      {{ t('shell.recharge') }}
    </button>
  </div>
</template>
