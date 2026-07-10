<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { UserCircle } from 'lucide-vue-next'
import { usePlatformAuthStore } from '../../../stores/platformAuth'
import PlatformLoginActions from '../../auth/PlatformLoginActions.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const platformAuth = usePlatformAuthStore()

const {
  platformAccountTitle,
  platformLogoutBusy,
  logoutPlatformAccount,
  loginPlatformAccount
} = props.form

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
  <div>
    <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
      <UserCircle class="w-4 h-4 text-accent" />
      {{ platformAuth.isStandalone ? '管理员账户' : '平台账户' }}
    </h3>
    <p class="mt-0.5 text-xs text-muted">
      {{ platformAuth.isStandalone ? '独立部署登录状态' : 'Pointer 平台登录状态' }}
    </p>
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
</template>
