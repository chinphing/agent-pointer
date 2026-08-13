<script setup lang="ts">
import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { AlertTriangle, UserCircle, WalletCards } from 'lucide-vue-next'
import { usePlatformAuthStore } from '../../../stores/platformAuth'
import { usePlatformBalance } from '../../../composables/usePlatformBalance'
import { openPlatformBillingPage } from '../../../lib/platformUrls'
import PlatformLoginActions from '../../auth/PlatformLoginActions.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const platformAuth = usePlatformAuthStore()
const { balance, exhausted, loading, lowBalance, visible: balanceVisible } = usePlatformBalance()

const balanceLabel = computed(() => {
  if (loading.value && balance.value == null) return '余额…'
  return balance.value == null ? '余额 --' : `${balance.value} 元`
})

const {
  platformAccountTitle,
  platformLogoutBusy,
  logoutPlatformAccount,
  loginPlatformAccount
} = props.form

const avatarInitial = computed(() => {
  const title = platformAccountTitle.value?.trim() ?? ''
  return title ? title.charAt(0).toUpperCase() : '?'
})

async function onPlatformLogin() {
  try {
    await loginPlatformAccount()
  } catch {
    /* error in store */
  }
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
}
</script>

<template>
  <div class="flex items-start justify-between gap-4">
    <div>
      <h3 class="text-lg font-semibold tracking-tight text-foreground">账户</h3>
      <p class="mt-1 text-sm text-muted">
        {{ platformAuth.isStandalone ? '管理登录凭据与应用偏好' : '查看余额、管理登录凭据与应用偏好' }}
      </p>
    </div>
    <div class="hidden sm:flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-accent/10 text-accent">
      <UserCircle class="h-5 w-5" aria-hidden="true" />
    </div>
  </div>

  <!-- 账户信息 + 余额（双列：用户左、余额右） -->
  <section class="mt-5 grid gap-4 lg:grid-cols-2">
    <!-- 账户信息 -->
    <div
      class="rounded-2xl border border-border panel p-5"
      :class="balanceVisible ? '' : 'lg:col-span-2'"
    >
      <div class="flex items-center gap-3">
        <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-[hsl(var(--card-elevated))] text-foreground">
          <span class="text-base font-semibold leading-none" aria-hidden="true">{{ avatarInitial }}</span>
        </div>
        <div class="min-w-0 flex-1">
          <p class="truncate text-base font-semibold text-foreground">{{ platformAccountTitle }}</p>
          <p class="mt-0.5 text-xs text-muted">
            {{ platformAuth.session.logged_in ? '凭据已安全保存在本机' : platformAuth.isStandalone ? '登录后可使用本服务' : '登录后可使用平台相关能力' }}
          </p>
        </div>
        <span
          v-if="platformAuth.session.logged_in"
          class="shrink-0 rounded-full border border-success/40 bg-success/15 px-2.5 py-1 text-xs font-medium text-success"
        >
          已登录
        </span>
      </div>

      <div class="mt-5 border-t border-border pt-4">
        <button
          v-if="platformAuth.session.logged_in"
          type="button"
          class="h-9 rounded-lg border border-destructive/60 px-4 text-sm text-destructive hover:bg-destructive/10 cursor-pointer transition-colors disabled:opacity-50"
          :disabled="platformLogoutBusy"
          @click="logoutPlatformAccount"
        >
          {{ platformLogoutBusy ? '退出中…' : '退出登录' }}
        </button>
        <PlatformLoginActions
          v-else
          variant="hero"
          :loading="platformAuth.loading"
          :error="platformAuth.error"
          @login="onPlatformLogin"
          @cancel="onPlatformLoginCancel"
        />
      </div>
    </div>

    <!-- 余额 -->
    <div
      v-if="balanceVisible"
      class="rounded-2xl border border-border panel p-5"
    >
      <div class="flex items-center gap-3">
        <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-[hsl(var(--card-elevated))] text-foreground">
          <AlertTriangle
            v-if="exhausted || lowBalance"
            class="h-5 w-5 text-warning"
            aria-hidden="true"
          />
          <WalletCards v-else class="h-5 w-5 opacity-70" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="truncate text-base font-semibold text-foreground">{{ balanceLabel }}</p>
          <p class="mt-0.5 text-xs text-muted">
            {{ exhausted ? '余额已用尽，请充值后继续使用' : lowBalance ? '余额较低，请及时充值' : '可用于平台模型与云主机服务' }}
          </p>
        </div>
      </div>

      <div class="mt-5 border-t border-border pt-4">
        <button
          type="button"
          class="h-9 rounded-lg border border-border px-4 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
          aria-label="前往账户充值"
          @click="openPlatformBillingPage"
        >
          充值
        </button>
      </div>
    </div>
  </section>
</template>
