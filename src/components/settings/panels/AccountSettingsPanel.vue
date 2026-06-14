<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { UserCircle } from 'lucide-vue-next'
import { usePlatformAuthStore } from '../../../stores/platformAuth'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const platformAuth = usePlatformAuthStore()

const s = useSettingsStore()
const {
  platformAccountTitle,
  platformLogoutBusy,
  logoutPlatformAccount,
  loginPlatformAccount
} = props.form
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <UserCircle class="w-4 h-4 text-accent" />平台账户
              </h3>
              <p class="mt-0.5 text-xs text-muted">Pointer 平台登录状态</p>
            </div>

            <div class="rounded-xl border border-border panel p-5 space-y-4">
              <div class="flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <p class="text-sm font-medium text-foreground">{{ platformAccountTitle }}</p>
                  <p v-if="!platformAuth.session.logged_in" class="mt-1 text-xs text-muted">
                    登录后可使用平台相关能力
                  </p>
                </div>
                <span
                  v-if="platformAuth.session.logged_in"
                  class="shrink-0 rounded-md border border-success/30 bg-success/10 px-2 py-0.5 text-[11px] text-success"
                >
                  已登录
                </span>
              </div>

              <div class="flex flex-wrap gap-2">
                <button
                  v-if="platformAuth.session.logged_in"
                  type="button"
                  class="h-8 px-4 rounded-lg border border-border text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
                  :disabled="platformLogoutBusy"
                  @click="logoutPlatformAccount"
                >
                  {{ platformLogoutBusy ? '退出中…' : '退出登录' }}
                </button>
                <button
                  v-else
                  type="button"
                  class="h-8 px-4 rounded-lg bg-accent text-sm font-medium text-white hover:opacity-95 cursor-pointer transition-opacity disabled:opacity-50"
                  :disabled="platformAuth.loading"
                  @click="loginPlatformAccount"
                >
                  {{ platformAuth.loading ? '等待授权…' : '浏览器登录' }}
                </button>
                <button
                  v-if="platformAuth.loading"
                  type="button"
                  class="h-8 px-4 rounded-lg border border-border text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
                  @click="platformAuth.cancelLogin()"
                >
                  取消
                </button>
              </div>
              <p
                v-if="platformAuth.error && !platformAuth.session.logged_in"
                class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
                role="alert"
              >
                {{ platformAuth.error }}
              </p>
            </div>

</template>
