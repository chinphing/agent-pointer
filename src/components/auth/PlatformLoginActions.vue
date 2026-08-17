<script setup lang="ts">
import { ExternalLink, Loader2, RefreshCw } from 'lucide-vue-next'
import { computed, onMounted, ref, watch } from 'vue'
import * as api from '../../lib/api'
import { isTauriRuntime } from '../../lib/runtime'
import { usePlatformAuthStore } from '../../stores/platformAuth'

withDefaults(
  defineProps<{
    loading: boolean
    error: string | null
    variant?: 'hero' | 'compact'
  }>(),
  { variant: 'hero' }
)

const emit = defineEmits<{
  (e: 'login'): void
  (e: 'cancel'): void
  (e: 'local-success'): void
}>()

const platformAuth = usePlatformAuthStore()
const isStandalone = computed(() => platformAuth.isStandalone)

const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaId = ref('')
const captchaSvg = ref('')
const captchaLoading = ref(false)
const localSubmitting = ref(false)
const localError = ref<string | null>(null)

const heroHintText = '将在系统浏览器中打开授权页面'

const displayError = computed(() => localError.value || null)

async function refreshCaptcha() {
  if (!isStandalone.value) return
  captchaLoading.value = true
  localError.value = null
  try {
    const res = await api.fetchLocalCaptcha()
    captchaId.value = res.captchaId
    captchaSvg.value = res.imageSvg
    captcha.value = ''
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    localError.value = msg.includes('local_auth_only_in_standalone')
      ? '当前不是独立部署模式'
      : '验证码加载失败，请重试'
    console.warn('fetchLocalCaptcha failed', e)
  } finally {
    captchaLoading.value = false
  }
}

async function submitLocalLogin() {
  if (localSubmitting.value || platformAuth.loading) return
  const u = username.value.trim()
  const p = password.value
  const c = captcha.value.trim()
  if (!u || !p) {
    localError.value = '请输入账号和密码'
    return
  }
  if (!captchaId.value || !c) {
    localError.value = '请输入验证码'
    return
  }
  localSubmitting.value = true
  localError.value = null
  try {
    await platformAuth.loginLocal({
      username: u,
      password: p,
      captchaId: captchaId.value,
      captcha: c
    })
    password.value = ''
    captcha.value = ''
    emit('local-success')
  } catch {
    await refreshCaptcha()
  } finally {
    localSubmitting.value = false
  }
}

onMounted(() => {
  void (async () => {
    if (!isTauriRuntime()) {
      await platformAuth.loadAuthMode()
    }
    if (isStandalone.value) void refreshCaptcha()
  })()
})

watch(isStandalone, standalone => {
  if (standalone) void refreshCaptcha()
})
</script>

<template>
  <div
    :class="
      variant === 'compact'
        ? 'inline-flex shrink-0 flex-col items-stretch gap-2'
        : 'flex flex-col items-center gap-2'
    "
  >
    <!-- Standalone: username / password / captcha -->
    <template v-if="isStandalone">
      <p
        v-if="displayError || error"
        class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
        :class="variant === 'compact' ? 'max-w-full' : 'max-w-sm'"
        role="alert"
      >
        {{ displayError || error }}
      </p>

      <form
        class="flex w-full flex-col gap-2"
        :class="variant === 'hero' ? 'max-w-sm' : 'min-w-[16rem]'"
        @submit.prevent="submitLocalLogin"
      >
        <input
          v-model="username"
          type="text"
          name="username"
          autocomplete="username"
          placeholder="账号"
          class="h-10 w-full rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          :disabled="localSubmitting || loading"
        />
        <input
          v-model="password"
          type="password"
          name="password"
          autocomplete="current-password"
          placeholder="密码"
          class="h-10 w-full rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          :disabled="localSubmitting || loading"
        />
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="relative flex h-10 w-[8.75rem] shrink-0 items-center justify-center overflow-hidden rounded-xl border border-border bg-muted/40 cursor-pointer hover:bg-hover disabled:opacity-50"
            :disabled="captchaLoading || localSubmitting || loading"
            title="点击刷新验证码"
            @click="refreshCaptcha"
          >
            <span
              v-if="captchaSvg"
              class="pointer-events-none block h-full w-full [&>svg]:h-full [&>svg]:w-full"
              v-html="captchaSvg"
            />
            <Loader2 v-else-if="captchaLoading" class="h-4 w-4 animate-spin text-muted" />
            <RefreshCw v-else class="h-4 w-4 text-muted" />
          </button>
          <input
            v-model="captcha"
            type="text"
            name="captcha"
            autocomplete="off"
            placeholder="验证码"
            class="h-10 min-w-0 flex-1 rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
            :disabled="localSubmitting || loading"
          />
        </div>
        <button
          type="submit"
          class="inline-flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-accent-foreground cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          :disabled="localSubmitting || loading"
        >
          <Loader2 v-if="localSubmitting || loading" class="h-4 w-4 shrink-0 animate-spin" />
          <span>{{ localSubmitting || loading ? '登录中…' : '登录' }}</span>
        </button>
      </form>
    </template>

    <!-- Platform OAuth -->
    <template v-else>
      <p
        v-if="error"
        class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
        :class="variant === 'compact' ? 'sm:max-w-[14rem]' : 'max-w-sm'"
        role="alert"
      >
        {{ error }}
      </p>

      <div
        v-if="variant === 'hero'"
        class="inline-flex flex-col-reverse items-center gap-2"
      >
        <p class="whitespace-nowrap text-center text-[11px] leading-relaxed text-muted">
          {{ loading ? '请在浏览器中完成授权' : heroHintText }}
        </p>
        <div class="flex w-[calc(100%+0.75rem)] items-center justify-center gap-2">
          <button
            type="button"
            class="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-accent-foreground cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            :disabled="loading"
            @click="emit('login')"
          >
            <Loader2 v-if="loading" class="h-4 w-4 shrink-0 animate-spin" />
            <ExternalLink v-else class="h-4 w-4 shrink-0 opacity-90" />
            <span>{{ loading ? '等待授权' : '浏览器登录' }}</span>
          </button>
          <button
            v-if="loading"
            type="button"
            class="inline-flex h-10 shrink-0 items-center justify-center rounded-lg border border-border px-4 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
            @click="emit('cancel')"
          >
            取消
          </button>
        </div>
      </div>

      <div
        v-else
        class="flex shrink-0 flex-wrap items-center justify-center gap-2"
      >
        <button
          type="button"
          class="inline-flex h-8 items-center justify-center gap-2 rounded-lg bg-accent px-3 text-sm font-medium text-accent-foreground cursor-pointer transition-all hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          :disabled="loading"
          @click="emit('login')"
        >
          <Loader2 v-if="loading" class="h-4 w-4 shrink-0 animate-spin" />
          <ExternalLink v-else class="h-4 w-4 shrink-0 opacity-90" />
          <span>{{ loading ? '等待授权' : '浏览器登录' }}</span>
        </button>
        <button
          v-if="loading"
          type="button"
          class="inline-flex h-8 shrink-0 items-center justify-center rounded-lg border border-border px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
          @click="emit('cancel')"
        >
          取消
        </button>
      </div>
    </template>
  </div>
</template>
