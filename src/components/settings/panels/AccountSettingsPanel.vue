<script setup lang="ts">
import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { AlertTriangle, Volume2, UserCircle, WalletCards } from 'lucide-vue-next'
import { usePlatformAuthStore } from '../../../stores/platformAuth'
import { useSettingsStore } from '../../../stores/settings'
import { usePlatformBalance } from '../../../composables/usePlatformBalance'
import { playTaskCompleteSound, primeTaskCompleteAudio } from '../../../lib/taskCompleteSound'
import { openPlatformBillingPage } from '../../../lib/platformUrls'
import PlatformLoginActions from '../../auth/PlatformLoginActions.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const { balance, exhausted, loading, lowBalance, visible: balanceVisible } = usePlatformBalance()
const soundSaving = ref(false)

const balanceToneClass = computed(() => {
  if (exhausted.value) return 'border-warning/40 bg-warning/10 text-warning'
  if (lowBalance.value) return 'border-warning/25 bg-warning/5 text-warning'
  return 'border-border bg-card text-foreground'
})

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

const playSoundOnFinish = ref(settings.userSettings.playSoundOnFinish !== false)

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

async function onPlaySoundToggle(checked: boolean) {
  playSoundOnFinish.value = checked
  soundSaving.value = true
  try {
    await settings.saveUser({ playSoundOnFinish: checked })
    console.info('[settings] playSoundOnFinish=%s', checked)
    if (checked) {
      primeTaskCompleteAudio()
      void playTaskCompleteSound()
    }
  } catch (err) {
    playSoundOnFinish.value = settings.userSettings.playSoundOnFinish !== false
    console.error('[settings] failed to save playSoundOnFinish', err)
  } finally {
    soundSaving.value = false
  }
}
</script>

<template>
  <div>
    <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
      <UserCircle class="w-4 h-4 text-accent" />
      账户
    </h3>
    <p class="mt-0.5 text-xs text-muted">
      {{ platformAuth.isStandalone ? '独立部署登录与凭据' : '余额、登录与平台凭据' }}
    </p>
  </div>

  <div
    v-if="balanceVisible"
    class="rounded-2xl border p-6"
    :class="balanceToneClass"
  >
    <div class="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between">
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-sm font-medium">
          <AlertTriangle
            v-if="exhausted || lowBalance"
            class="h-4 w-4 shrink-0"
            aria-hidden="true"
          />
          <WalletCards v-else class="h-4 w-4 shrink-0 opacity-70" aria-hidden="true" />
          <span>账户余额</span>
        </div>
        <p class="mt-3 text-4xl font-semibold tracking-tight tabular-nums text-foreground sm:text-5xl">
          {{ balanceLabel }}
        </p>
        <p class="mt-2 text-xs opacity-80">
          {{ exhausted ? '余额已用尽，请充值后继续使用' : lowBalance ? '余额较低，请及时充值' : '可用于平台模型与云主机服务' }}
        </p>
      </div>
      <button
        type="button"
        class="h-10 shrink-0 rounded-lg bg-accent px-5 text-sm font-medium text-white hover:opacity-95 cursor-pointer"
        aria-label="前往账户充值"
        @click="openPlatformBillingPage"
      >
        充值
      </button>
    </div>
  </div>

  <div class="rounded-xl border border-border panel p-5 space-y-4">
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <p class="text-sm font-medium text-foreground">{{ platformAccountTitle }}</p>
        <p v-if="!platformAuth.session.logged_in" class="mt-1 text-xs text-muted">
          {{ platformAuth.isStandalone ? '登录后可使用本服务' : '登录后可使用平台相关能力' }}
        </p>
      </div>
      <span
        v-if="platformAuth.session.logged_in"
        class="shrink-0 rounded-md border border-success/30 bg-success/10 px-2 py-0.5 text-[11px] text-success"
      >
        已登录
      </span>
    </div>

    <div v-if="platformAuth.session.logged_in" class="flex flex-wrap gap-2">
      <button
        type="button"
        class="h-8 px-4 rounded-lg border border-border text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
        :disabled="platformLogoutBusy"
        @click="logoutPlatformAccount"
      >
        {{ platformLogoutBusy ? '退出中…' : '退出登录' }}
      </button>
    </div>
    <div v-else>
      <PlatformLoginActions
        variant="hero"
        :loading="platformAuth.loading"
        :error="platformAuth.error"
        @login="onPlatformLogin"
        @cancel="onPlatformLoginCancel"
      />
    </div>
  </div>

  <div>
    <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
      <Volume2 class="w-4 h-4 text-accent" />
      通知
    </h3>
    <p class="mt-0.5 text-xs text-muted">任务完成时的提示</p>
  </div>

  <div class="rounded-xl border border-border panel p-5">
    <div class="flex items-center justify-between gap-4">
      <div class="min-w-0">
        <p class="text-sm font-medium text-foreground">完成时播放提示音</p>
        <p class="mt-1 text-xs text-muted">对话回合结束时播放短促提示音</p>
      </div>
      <label class="relative inline-flex items-center cursor-pointer shrink-0">
        <input
          type="checkbox"
          class="sr-only peer"
          :checked="playSoundOnFinish"
          :disabled="soundSaving"
          @change="onPlaySoundToggle(($event.target as HTMLInputElement).checked)"
        />
        <div class="settings-toggle-track" />
      </label>
    </div>
  </div>
</template>
