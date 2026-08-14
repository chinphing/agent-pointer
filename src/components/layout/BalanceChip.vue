<script setup lang="ts">
import { AlertTriangle, WalletCards } from 'lucide-vue-next'
import { computed } from 'vue'
import { usePlatformBalance } from '../../composables/usePlatformBalance'
import { openPlatformBillingPage } from '../../lib/platformUrls'

const { balance, exhausted, loading, lowBalance, visible } = usePlatformBalance()

const toneClass = computed(() => {
  if (exhausted.value) return 'border-warning/40 bg-warning/10 text-warning'
  if (lowBalance.value) return 'border-warning/25 bg-warning/5 text-warning'
  return 'border-border bg-card text-foreground'
})

const balanceLabel = computed(() => {
  if (loading.value && balance.value == null) return '余额…'
  return balance.value == null ? '余额 --' : `余额 ${balance.value} 元`
})
</script>

<template>
  <div
    v-if="visible"
    class="inline-flex h-7 shrink-0 items-center gap-1 rounded-full border pl-2 text-xs shadow-sm"
    :class="toneClass"
    :title="exhausted ? '账户余额已用尽，请充值后继续使用' : lowBalance ? '账户余额较低，请及时充值' : undefined"
  >
    <AlertTriangle v-if="exhausted || lowBalance" class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
    <WalletCards v-else class="h-3.5 w-3.5 shrink-0 text-muted" aria-hidden="true" />
    <span class="tabular-nums whitespace-nowrap">{{ balanceLabel }}</span>
    <button
      type="button"
      class="h-6 rounded-full border border-border/80 bg-background px-2 text-[11px] font-medium text-foreground hover:bg-hover cursor-pointer"
      aria-label="前往账户充值"
      @click="openPlatformBillingPage"
    >
      充值
    </button>
  </div>
</template>
