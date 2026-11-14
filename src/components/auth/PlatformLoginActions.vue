<script setup lang="ts">
import {
  Building2,
  ExternalLink,
  Globe,
  KeyRound,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Ticket,
  UserRound
} from 'lucide-vue-next'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
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

const { t } = useI18n()
const platformAuth = usePlatformAuthStore()
const isTauri = isTauriRuntime()

/** Buttons for the redirect providers; the ticket entry is entered from outside. */
const buttonProviders = computed(() => platformAuth.loginButtonProviders)
const hasLocalPassword = computed(() => platformAuth.hasLocalPasswordProvider)

const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaId = ref('')
const captchaSvg = ref('')
const captchaLoading = ref(false)
const submitting = ref(false)
const pendingProvider = ref<string | null>(null)
const localError = ref<string | null>(null)

const displayError = computed(() => localError.value || null)

/**
 * Icon per server-provided name. Unknown names fall back to the first letter of
 * the display name, so a new provider never renders an empty button.
 */
const PROVIDER_ICONS = {
  building: Building2,
  key: KeyRound,
  ticket: Ticket,
  user: UserRound,
  shield: ShieldCheck,
  globe: Globe
} as const

function providerIcon(name?: string) {
  const key = (name ?? '').trim().toLowerCase()
  return PROVIDER_ICONS[key as keyof typeof PROVIDER_ICONS] ?? null
}

function providerInitial(provider: { display_name: string; id: string }) {
  const label = (provider.display_name || provider.id).trim()
  return label ? label.charAt(0).toUpperCase() : '?'
}

async function refreshCaptcha() {
  if (isTauri || !hasLocalPassword.value) return
  captchaLoading.value = true
  localError.value = null
  try {
    const res = await platformAuth.requestLocalPasswordChallenge()
    captchaId.value = res.captchaId
    captchaSvg.value = res.imageSvg
    captcha.value = ''
  } catch (e) {
    localError.value = t('auth.captchaLoadFailed')
    console.warn('requestLocalPasswordChallenge failed', e)
  } finally {
    captchaLoading.value = false
  }
}

async function submitLocalLogin() {
  if (submitting.value || platformAuth.loading) return
  const u = username.value.trim()
  const p = password.value
  const c = captcha.value.trim()
  if (!u || !p) {
    localError.value = t('auth.needCredentials')
    return
  }
  if (!captchaId.value || !c) {
    localError.value = t('auth.needCaptcha')
    return
  }
  submitting.value = true
  localError.value = null
  try {
    await platformAuth.loginLocalPassword({
      username: u,
      password: p,
      captchaId: captchaId.value,
      captcha: c
    })
    password.value = ''
    captcha.value = ''
    emit('local-success')
  } catch (e) {
    // The store already mapped the code to text; ask for a fresh captcha.
    const message = platformAuth.error
    console.warn('local password login failed', e)
    await refreshCaptcha()
    localError.value = message || localError.value
  } finally {
    submitting.value = false
  }
}

async function startProvider(providerId: string) {
  if (pendingProvider.value || platformAuth.loading) return
  pendingProvider.value = providerId
  localError.value = null
  try {
    await platformAuth.loginProvider(providerId)
  } catch (e) {
    localError.value = platformAuth.error
    console.warn('provider login failed', providerId, e)
  } finally {
    pendingProvider.value = null
  }
}

onMounted(() => {
  void (async () => {
    if (isTauri) return
    if (platformAuth.providers.length === 0) await platformAuth.loadAuthProviders()
    if (hasLocalPassword.value) void refreshCaptcha()
  })()
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
    <p
      v-if="displayError || error"
      class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
      :class="variant === 'compact' ? 'max-w-full' : 'max-w-sm'"
      role="alert"
    >
      {{ displayError || error }}
    </p>

    <!-- Desktop: the Tauri command owns the browser round-trip. -->
    <div
      v-if="isTauri"
      class="flex items-center justify-center gap-2"
      :class="variant === 'hero' ? 'w-[calc(100%+0.75rem)]' : 'shrink-0'"
    >
      <button
        type="button"
        class="inline-flex items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-accent-foreground cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        :class="variant === 'hero' ? 'h-10 flex-1' : 'h-8'"
        :disabled="loading"
        @click="emit('login')"
      >
        <Loader2 v-if="loading" class="h-4 w-4 shrink-0 animate-spin" />
        <ExternalLink v-else class="h-4 w-4 shrink-0 opacity-90" />
        <span>{{ loading ? t('auth.waitingAuth') : t('auth.browserLogin') }}</span>
      </button>
      <button
        v-if="loading"
        type="button"
        class="inline-flex h-10 shrink-0 items-center justify-center rounded-lg border border-border px-4 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
        @click="emit('cancel')"
      >
        {{ t('common.cancel') }}
      </button>
    </div>

    <!-- Web: one entry per provider from the server registry. -->
    <template v-else>
      <form
        v-if="hasLocalPassword"
        class="flex w-full flex-col gap-2"
        :class="variant === 'hero' ? 'max-w-sm' : 'min-w-[16rem]'"
        @submit.prevent="submitLocalLogin"
      >
        <input
          v-model="username"
          type="text"
          name="username"
          autocomplete="username"
          :placeholder="t('auth.username')"
          class="h-10 w-full rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          :disabled="submitting || loading"
        />
        <input
          v-model="password"
          type="password"
          name="password"
          autocomplete="current-password"
          :placeholder="t('auth.password')"
          class="h-10 w-full rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          :disabled="submitting || loading"
        />
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="relative flex h-10 w-[8.75rem] shrink-0 items-center justify-center overflow-hidden rounded-xl border border-border bg-muted/40 cursor-pointer hover:bg-hover disabled:opacity-50"
            :disabled="captchaLoading || submitting || loading"
            :title="t('auth.refreshCaptcha')"
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
            :placeholder="t('auth.captcha')"
            class="h-10 min-w-0 flex-1 rounded-xl border border-border bg-background px-3 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
            :disabled="submitting || loading"
          />
        </div>
        <button
          type="submit"
          class="inline-flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-accent-foreground cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          :disabled="submitting || loading"
        >
          <Loader2 v-if="submitting || loading" class="h-4 w-4 shrink-0 animate-spin" />
          <span>{{ submitting || loading ? t('auth.signingIn') : t('auth.signIn') }}</span>
        </button>
      </form>

      <div
        v-if="buttonProviders.length"
        class="flex gap-2"
        :class="variant === 'hero' ? 'w-full max-w-sm flex-col' : 'shrink-0 flex-wrap justify-center'"
      >
        <button
          v-for="provider in buttonProviders"
          :key="provider.id"
          type="button"
          class="inline-flex items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-accent-foreground cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          :class="variant === 'hero' ? 'h-10 w-full' : 'h-8'"
          :disabled="loading || pendingProvider !== null"
          @click="startProvider(provider.id)"
        >
          <Loader2
            v-if="pendingProvider === provider.id"
            class="h-4 w-4 shrink-0 animate-spin"
          />
          <template v-else>
            <component
              :is="providerIcon(provider.icon)"
              v-if="providerIcon(provider.icon)"
              class="h-4 w-4 shrink-0 opacity-90"
            />
            <span
              v-else
              class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent-foreground/20 text-[10px] font-semibold leading-none"
              aria-hidden="true"
            >
              {{ providerInitial(provider) }}
            </span>
          </template>
          <span class="truncate">{{ provider.display_name || t('auth.browserLogin') }}</span>
        </button>
      </div>

      <p
        v-if="!hasLocalPassword && !buttonProviders.length"
        class="text-xs leading-snug text-muted"
        :class="variant === 'hero' ? 'max-w-sm text-center' : 'max-w-[14rem]'"
      >
        {{ t('auth.noProviders') }}
      </p>

      <p
        v-if="isTauri && variant === 'hero' && (hasLocalPassword || buttonProviders.length)"
        class="whitespace-nowrap text-center text-[11px] leading-relaxed text-muted"
      >
        {{ loading ? t('auth.completeInBrowser') : t('auth.heroHint') }}
      </p>
    </template>
  </div>
</template>
