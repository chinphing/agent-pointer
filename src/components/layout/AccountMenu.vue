<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  AlertTriangle,
  Download,
  Loader2,
  LogIn,
  LogOut,
  Monitor,
  Moon,
  Palette,
  RefreshCw,
  Settings,
  Sun,
  WalletCards,
  X
} from 'lucide-vue-next'
import { applyTheme } from '../../lib/theme'
import type { ThemePreference } from '../../types/chat'
import { useSettingsStore } from '../../stores/settings'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { usePlatformBalance } from '../../composables/usePlatformBalance'
import { useAppUpdater } from '../../composables/useAppUpdater'
import { openPlatformBillingPage } from '../../lib/platformUrls'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'

const emit = defineEmits<{
  (e: 'open-settings', section?: string): void
}>()

const s = useSettingsStore()
const platformAuth = usePlatformAuthStore()
const updater = useAppUpdater()
const {
  balance,
  loading: balanceLoading,
  visible: balanceVisible,
  lowBalance,
  exhausted,
  refresh: refreshBalance
} = usePlatformBalance()

const menuOpen = ref(false)
const showLogin = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const menuStyle = ref({ left: '8px', bottom: '0px' })

const themeOptions = ['light', 'dark', 'system'] as const

const platformAccountTitle = computed(() => {
  if (!platformAuth.session.logged_in) return '未登录'
  return platformAuth.session.user_nickname?.trim() || '已登录'
})

const platformLogoutBusy = ref(false)

const balanceLabel = computed(() => {
  if (balanceLoading.value && balance.value == null) return '…'
  return balance.value == null ? '--' : `${balance.value} 元`
})

const theme = computed<ThemePreference>(() => (s.settings.theme as ThemePreference) || 'system')

function themeLabel(t: ThemePreference): string {
  if (t === 'light') return '浅色'
  if (t === 'dark') return '深色'
  return '跟随系统'
}

const checkUpdateLabel = computed(() => {
  if (updater.updating.value || updater.downloading.value) return '更新中…'
  if (updater.checking.value) return '检查中…'
  if (updater.statusMessage.value) return updater.statusMessage.value
  return '检查更新'
})

const readyUpdateLabel = computed(() => {
  if (updater.updating.value || updater.downloading.value) return '更新中…'
  if (updater.updateReady.value) return '新版本已就绪，点击更新'
  if (updater.updateVersion.value) return `发现新版本 v${updater.updateVersion.value}，点击更新`
  return '立即更新'
})

async function saveThemeChoice(t: ThemePreference) {
  applyTheme(t)
  s.settings.theme = t
  s.userSettings.theme = t
  try {
    await s.saveUser({ theme: t })
  } catch (e) {
    console.error('[account-menu] failed to save theme', e)
  }
}

async function applyThemeChoice(t: ThemePreference) {
  await saveThemeChoice(t)
  menuOpen.value = false
}

function toggleMenu() {
  if (menuOpen.value) {
    menuOpen.value = false
    return
  }
  const rect = triggerRef.value?.getBoundingClientRect()
  menuStyle.value = {
    left: `${Math.max(8, rect?.left ?? 8)}px`,
    bottom: `${window.innerHeight - (rect?.top ?? window.innerHeight) + 8}px`
  }
  menuOpen.value = true
  void refreshBalance()
}

async function onPlatformLogin() {
  // Standalone 本地账号密码需要表单，弹窗内嵌 PlatformLoginActions。
  if (platformAuth.isStandalone) {
    menuOpen.value = false
    showLogin.value = true
    return
  }
  try {
    await platformAuth.login()
  } catch {
    /* error in store */
  }
}

async function onPlatformLogout() {
  platformLogoutBusy.value = true
  try {
    await platformAuth.logout()
  } catch (e) {
    console.error('[account-menu] platform logout failed', e)
  } finally {
    platformLogoutBusy.value = false
  }
  menuOpen.value = false
}

watch(
  () => platformAuth.session.logged_in,
  loggedIn => {
    if (loggedIn) showLogin.value = false
  }
)

function onOutsideClick(event: MouseEvent) {
  const target = event.target
  if (!(target instanceof Element)) return
  if (!target.closest('.account-menu-trigger, .account-menu-panel')) {
    menuOpen.value = false
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  menuOpen.value = false
}

onMounted(() => {
  document.addEventListener('mousedown', onOutsideClick)
  document.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onOutsideClick)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div class="relative shrink-0">
    <!-- 触发器：App 图标 + 用户名 + 上箭头（左下角用户胶囊） -->
    <button
      ref="triggerRef"
      type="button"
      class="account-menu-trigger flex h-8 max-w-[11rem] items-center gap-1.5 rounded-lg border border-transparent px-1.5 text-left hover:bg-hover transition-colors cursor-pointer"
      :aria-expanded="menuOpen"
      aria-haspopup="menu"
      @click="toggleMenu"
    >
      <img
        src="/app-icon.png"
        alt=""
        class="h-5 w-5 shrink-0 rounded-full object-cover"
        draggable="false"
      >
      <span class="min-w-0 flex-1 truncate text-[13px] text-foreground/90">{{ platformAccountTitle }}</span>
    </button>

    <!-- 下拉菜单（左下角向上弹出） -->
    <Teleport to="body">
      <div
        v-if="menuOpen"
        class="account-menu-panel fixed z-[310] w-60 rounded-xl border border-border bg-card p-1.5 shadow-xl"
        role="menu"
        :style="menuStyle"
      >
        <!-- 余额：整行点击前往充值 -->
        <button
          v-if="balanceVisible"
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left hover:bg-hover transition-colors cursor-pointer"
          :aria-label="`余额 ${balanceLabel}，前往充值`"
          @click="openPlatformBillingPage"
        >
          <AlertTriangle v-if="exhausted || lowBalance" class="h-[18px] w-[18px] shrink-0 text-warning" aria-hidden="true" />
          <WalletCards v-else class="h-[18px] w-[18px] shrink-0 text-muted" aria-hidden="true" />
          <span class="min-w-0 flex-1 text-[13px] text-foreground">余额</span>
          <span class="shrink-0 text-[13px] font-semibold text-foreground/70">{{ balanceLabel }}</span>
        </button>

        <!-- 外观：固定调色板图标 + 标签 + 当前主题图标；hover 时中间滑入纯图标抽屉（不超出面板） -->
        <div class="group relative flex items-center gap-2.5 rounded-lg px-2.5 py-2 transition-colors hover:bg-hover">
          <Palette class="h-[18px] w-[18px] shrink-0 text-muted" aria-hidden="true" />
          <span class="flex-1 text-[13px] text-foreground">外观</span>
          <!-- 中间抽屉：hover 时三个图标逐项滑入（抽屉逐步推出效果） -->
          <div class="pointer-events-none absolute right-2 top-1/2 z-20 flex -translate-y-1/2 items-center gap-0.5 p-0.5 group-hover:pointer-events-auto">
            <button
              v-for="(t, i) in themeOptions"
              :key="t"
              type="button"
              role="menuitemradio"
              class="flex h-7 w-7 items-center justify-center rounded-md opacity-0 translate-x-2 transition-all duration-200 cursor-pointer"
              :class="[
                i === 0 ? 'group-hover:translate-x-0 group-hover:opacity-100' : '',
                i === 1 ? 'delay-75 group-hover:translate-x-0 group-hover:opacity-100' : '',
                i === 2 ? 'delay-150 group-hover:translate-x-0 group-hover:opacity-100' : '',
                theme === t ? 'bg-hover text-foreground' : 'text-muted hover:bg-hover hover:text-foreground'
              ]"
              :title="themeLabel(t)"
              :aria-label="themeLabel(t)"
              :aria-checked="theme === t"
              @click="saveThemeChoice(t)"
            >
              <component
                :is="t === 'light' ? Sun : t === 'dark' ? Moon : Monitor"
                class="h-4 w-4"
                aria-hidden="true"
              />
            </button>
          </div>
          <!-- 右侧当前主题指示（hover 时淡出，避免与抽屉重复） -->
          <button
            type="button"
            class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border text-muted transition-all duration-150 group-hover:opacity-0 cursor-pointer"
            :title="`当前：${themeLabel(theme)}`"
            :aria-label="`当前：${themeLabel(theme)}，悬停选择主题`"
            @click.stop
          >
            <component
              :is="theme === 'light' ? Sun : theme === 'dark' ? Moon : Monitor"
              class="h-4 w-4"
              aria-hidden="true"
            />
          </button>
        </div>

        <!-- 设置 -->
        <button
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] text-foreground hover:bg-hover transition-colors cursor-pointer"
          @click="menuOpen = false; emit('open-settings', 'assistant')"
        >
          <Settings class="h-[18px] w-[18px] shrink-0 text-muted" aria-hidden="true" />
          <span class="flex-1">设置</span>
        </button>

        <!-- 检查更新 -->
        <button
          v-if="!updater.updateAvailable.value && !updater.updateReady.value"
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] text-foreground hover:bg-hover transition-colors cursor-pointer disabled:opacity-50"
          :disabled="updater.checking.value || updater.updating.value || updater.downloading.value"
          @click="updater.checkForUpdateManual()"
        >
          <Loader2 v-if="updater.checking.value" class="h-[18px] w-[18px] shrink-0 animate-spin text-muted" />
          <RefreshCw v-else class="h-[18px] w-[18px] shrink-0 text-muted" aria-hidden="true" />
          <span class="flex-1">{{ checkUpdateLabel }}</span>
        </button>
        <button
          v-else
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] text-accent hover:bg-accent/10 transition-colors cursor-pointer disabled:opacity-50"
          :disabled="updater.updating.value || updater.downloading.value"
          @click="updater.applyUpdateNow()"
        >
          <Loader2
            v-if="updater.updating.value || updater.downloading.value"
            class="h-[18px] w-[18px] shrink-0 animate-spin text-accent"
          />
          <Download v-else class="h-[18px] w-[18px] shrink-0 text-accent" aria-hidden="true" />
          <span class="flex-1">{{ readyUpdateLabel }}</span>
        </button>

        <div class="my-1 h-px bg-border/60" />

        <!-- 退出 / 登录 -->
        <button
          v-if="platformAuth.session.logged_in"
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] text-danger hover:bg-danger/10 transition-colors cursor-pointer disabled:opacity-50"
          :disabled="platformLogoutBusy"
          @click="onPlatformLogout"
        >
          <Loader2 v-if="platformLogoutBusy" class="h-[18px] w-[18px] shrink-0 animate-spin" />
          <LogOut v-else class="h-[18px] w-[18px] shrink-0" aria-hidden="true" />
          <span class="flex-1">{{ platformLogoutBusy ? '退出中…' : '退出登录' }}</span>
        </button>
        <button
          v-else
          type="button"
          role="menuitem"
          class="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] text-foreground hover:bg-hover transition-colors cursor-pointer"
          @click="onPlatformLogin"
        >
          <LogIn class="h-[18px] w-[18px] shrink-0 text-muted" aria-hidden="true" />
          <span class="flex-1">{{ platformAuth.loading ? '等待授权…' : '登录' }}</span>
        </button>
      </div>
    </Teleport>

    <!-- standalone 本地登录弹窗 -->
    <Teleport to="body">
      <div
        v-if="showLogin"
        class="fixed inset-0 z-[400] flex items-center justify-center bg-foreground/32 p-4"
        @click.self="showLogin = false"
      >
        <section
          class="w-full max-w-sm rounded-xl border border-border bg-card p-5 shadow-2xl"
          role="dialog"
          aria-modal="true"
          aria-labelledby="account-menu-login-title"
        >
          <div class="mb-3 flex items-center justify-between">
            <h2 id="account-menu-login-title" class="text-sm font-semibold text-foreground">登录</h2>
            <button type="button" class="chrome-icon-btn" title="关闭" aria-label="关闭" @click="showLogin = false">
              <X class="w-4 h-4" />
            </button>
          </div>
          <PlatformLoginActions
            variant="hero"
            :loading="platformAuth.loading"
            :error="platformAuth.error"
            @login="onPlatformLogin"
            @cancel="platformAuth.cancelLogin"
            @local-success="showLogin = false"
          />
        </section>
      </div>
    </Teleport>
  </div>
</template>
