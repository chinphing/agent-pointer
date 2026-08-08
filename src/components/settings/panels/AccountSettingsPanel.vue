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

  <section class="grid gap-4 lg:grid-cols-3">
    <div
      v-if="balanceVisible"
      class="rounded-2xl border p-6 lg:col-span-2"
      :class="balanceToneClass"
    >
      <div class="flex h-full flex-col justify-between gap-8">
        <div>
          <div class="flex items-center gap-2 text-sm font-medium">
            <AlertTriangle
              v-if="exhausted || lowBalance"
              class="h-4 w-4 shrink-0"
              aria-hidden="true"
            />
            <WalletCards v-else class="h-4 w-4 shrink-0 opacity-70" aria-hidden="true" />
            <span>可用余额</span>
          </div>
          <p class="mt-4 text-4xl font-semibold tracking-tight tabular-nums text-foreground sm:text-5xl">
            {{ balanceLabel }}
          </p>
          <p class="mt-2 text-sm opacity-80">
            {{ exhausted ? '余额已用尽，请充值后继续使用' : lowBalance ? '余额较低，请及时充值' : '可用于平台模型与云主机服务' }}
          </p>
        </div>
        <div>
          <button
            type="button"
            class="h-10 rounded-lg bg-accent px-5 text-sm font-medium text-white hover:opacity-95 cursor-pointer"
            aria-label="前往账户充值"
            @click="openPlatformBillingPage"
          >
            充值
          </button>
        </div>
      </div>
    </div>

    <div
      class="rounded-2xl border border-border panel p-5"
      :class="balanceVisible ? '' : 'lg:col-span-3'"
    >
      <div class="flex items-start justify-between gap-4">
        <div class="min-w-0">
          <p class="text-xs font-medium uppercase tracking-wide text-muted">登录状态</p>
          <p class="mt-2 text-base font-semibold text-foreground">{{ platformAccountTitle }}</p>
          <p v-if="!platformAuth.session.logged_in" class="mt-1 text-sm text-muted">
            {{ platformAuth.isStandalone ? '登录后可使用本服务' : '登录后可使用平台相关能力' }}
          </p>
          <p v-else class="mt-1 text-sm text-muted">凭据已安全保存在本机</p>
        </div>
        <span
          v-if="platformAuth.session.logged_in"
          class="shrink-0 rounded-full border border-success/30 bg-success/10 px-2.5 py-1 text-xs font-medium text-success"
        >
          已登录
        </span>
      </div>

      <div v-if="platformAuth.session.logged_in" class="mt-6 border-t border-border pt-4">
        <button
          type="button"
          class="h-9 rounded-lg border border-border px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="platformLogoutBusy"
          @click="logoutPlatformAccount"
        >
          {{ platformLogoutBusy ? '退出中…' : '退出登录' }}
        </button>
      </div>
      <div v-else class="mt-5">
        <PlatformLoginActions
          variant="hero"
          :loading="platformAuth.loading"
          :error="platformAuth.error"
          @login="onPlatformLogin"
          @cancel="onPlatformLoginCancel"
        />
      </div>
    </div>
  </section>

  <section class="rounded-2xl border border-border panel divide-y divide-border">
    <div class="flex items-center justify-between gap-4 px-5 py-4">
      <div class="flex items-center gap-3">
        <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent/10 text-accent">
          <Volume2 class="h-4 w-4" aria-hidden="true" />
        </div>
        <div>
          <h4 class="text-sm font-semibold text-foreground">通知与偏好</h4>
          <p class="mt-0.5 text-xs text-muted">控制任务完成后的提示方式</p>
        </div>
      </div>
    </div>
    <div class="flex items-center justify-between gap-4 px-5 py-4">
      <div class="min-w-0">
        <p class="text-sm font-medium text-foreground">完成时播放提示音</p>
        <p class="mt-1 text-sm text-muted">对话回合结束时播放短促提示音</p>
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
  </section>
</template>
