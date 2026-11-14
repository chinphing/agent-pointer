<script setup lang="ts">
/**
 * Standalone web login page (`/login`).
 *
 * Rendering comes from the server's provider registry:
 * - one entry → that entry only, no subtitle, no "or";
 * - several → the `primary` entry (solid) on top, the rest below;
 * - `local_password` renders the captcha form, never a button;
 * - `token_lookup` never appears (that entry is entered from the portal).
 *
 * Interaction contract (each piece is demoable):
 * - focus lands on the username field (form primary) or the primary button;
 * - collapsible regions animate and move focus to the revealed content;
 * - the password field can be revealed; empty fields get inline errors;
 * - errors are announced politely (aria-live) without stealing focus;
 * - a 429 starts a 60s cooldown shown on the button;
 * - providers load behind a skeleton; Esc collapses and restores focus.
 */
import {
  ArrowLeft,
  Building2,
  Ellipsis,
  Eye,
  EyeOff,
  Globe,
  KeyRound,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Ticket,
  UserRound
} from 'lucide-vue-next'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { AuthProvider } from '../../lib/api'
import { usePlatformAuthStore } from '../../stores/platformAuth'

const { t } = useI18n()
const platformAuth = usePlatformAuthStore()

const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaId = ref('')
const captchaSvg = ref('')
const captchaLoading = ref(false)
const submitting = ref(false)
const pendingProvider = ref<string | null>(null)
const localError = ref<string | null>(null)
/** Secondary `local_password` starts collapsed: the form opens on click. */
const localFormOpen = ref(false)
const otherMethodsOpen = ref(false)
const passwordVisible = ref(false)
const providersLoading = ref(false)
const fieldErrors = ref<{ username?: string; password?: string; captcha?: string }>({})
/** Epoch ms when the rate-limit cooldown ends (0 = no cooldown). */
const cooldownUntil = ref(0)
const clock = ref(Date.now())
let cooldownTimer: ReturnType<typeof setInterval> | null = null

const usernameEl = ref<HTMLInputElement | null>(null)
const passwordEl = ref<HTMLInputElement | null>(null)
const captchaEl = ref<HTMLInputElement | null>(null)
const primaryButtonEl = ref<HTMLButtonElement | null>(null)
const localTriggerEl = ref<HTMLButtonElement | null>(null)
const otherTriggerEl = ref<HTMLButtonElement | null>(null)
const firstOtherEl = ref<HTMLButtonElement | null>(null)

const entries = computed(() => platformAuth.loginPageProviders)
const primary = computed(() => platformAuth.primaryLoginProvider)
const secondaries = computed(() => platformAuth.secondaryLoginProviders)
/** One entry → no subtitle (nothing to choose). */
const showSubtitle = computed(() => entries.value.length >= 2)
const localIsPrimary = computed(() => primary.value?.kind === 'local_password')
const localSecondary = computed(
  () => secondaries.value.find(provider => provider.kind === 'local_password') ?? null
)
const ssoSecondaries = computed(() =>
  secondaries.value.filter(provider => provider.kind !== 'local_password')
)
const showOr = computed(() => secondaries.value.length > 0 && primary.value != null)
const groupOtherMethods = computed(() => ssoSecondaries.value.length >= 2)
const showLocalForm = computed(() => localIsPrimary.value || localFormOpen.value)
/**
 * The user picked the secondary `local_password` entry: the page then shows the
 * form plus a way back, and hides every other entry.
 */
const focusedLocalForm = computed(() => localFormOpen.value && localSecondary.value != null)
const localProvider = computed<AuthProvider | null>(
  () => (localIsPrimary.value ? primary.value : localSecondary.value) ?? null
)

/** Seconds left in the 429 cooldown (0 when free to retry). */
const cooldownLeft = computed(() => {
  const ms = cooldownUntil.value - clock.value
  return ms > 0 ? Math.ceil(ms / 1000) : 0
})
const busy = computed(
  () => submitting.value || platformAuth.loading || cooldownLeft.value > 0
)
const displayError = computed(() => localError.value || platformAuth.error || null)
const showSkeleton = computed(
  () => providersLoading.value && entries.value.length === 0
)

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

function providerInitial(provider: AuthProvider) {
  const label = (provider.display_name || provider.id).trim()
  return label ? label.charAt(0).toUpperCase() : '?'
}

/** `display_name` is a noun; the button always reads `{name}登录`. */
function buttonLabel(provider: AuthProvider) {
  const raw = (provider.display_name || provider.id).trim()
  const noun = raw.replace(/\s*(登录|登入|sign\s*in|login|log\s*in)$/i, '').trim() || raw
  return t('auth.page.button', { name: noun })
}

function isRateLimitMessage(message: string) {
  const msg = message.toLowerCase()
  return (
    msg.includes('rate_limited') ||
    msg.includes('too_many_attempts') ||
    msg.includes('login_capacity_reached') ||
    /\b429\b/.test(msg)
  )
}

function startCooldown(seconds = 60) {
  cooldownUntil.value = Date.now() + seconds * 1000
  clock.value = Date.now()
  if (cooldownTimer != null) return
  cooldownTimer = setInterval(() => {
    clock.value = Date.now()
    if (cooldownUntil.value <= clock.value && cooldownTimer != null) {
      clearInterval(cooldownTimer)
      cooldownTimer = null
      cooldownUntil.value = 0
      // The wait is over: drop the cooldown clause, keep the original message.
      if (baseErrorMessage != null) localError.value = baseErrorMessage
    }
  }, 250)
}

/** Message without the cooldown clause, restored once the wait is over. */
let baseErrorMessage: string | null = null

/** Drop a trailing "请重试" so the cooldown clause does not repeat it. */
function stripRetryTail(text: string) {
  return text
    .replace(/[，,、]?\s*(请稍后重试|请重试|please\s+try\s+again|please\s+retry|try\s+again)\s*$/i, '')
    .replace(/[;；,，、—–-]+\s*$/, '')
    .trim()
}

/** Errors are announced, never focused (field validation is the exception). */
function reportError(message: string | null, raw?: string) {
  baseErrorMessage = message
  localError.value = message
  if (raw && isRateLimitMessage(raw)) {
    startCooldown(60)
    // One sentence: "验证码加载失败，请 60 秒后重试" — not two "重试" clauses.
    const base = stripRetryTail(message ?? t('auth.loginFailed'))
    localError.value = t('auth.page.retryAfter', { message: base, n: 60 })
  }
}

/** `{name}登录` plus the cooldown clause, as one string (no stray whitespace). */
function withCooldown(label: string) {
  const seconds = cooldownLeft.value
  return seconds > 0 ? `${label}${t('auth.page.cooldownSuffix', { n: seconds })}` : label
}

function clearFieldErrors() {
  fieldErrors.value = {}
}

function focusFirstInvalid() {
  const order: Array<[keyof typeof fieldErrors.value, HTMLInputElement | null]> = [
    ['username', usernameEl.value],
    ['password', passwordEl.value],
    ['captcha', captchaEl.value]
  ]
  for (const [key, el] of order) {
    if (fieldErrors.value[key]) {
      el?.focus()
      return
    }
  }
}

async function refreshCaptcha() {
  captchaLoading.value = true
  localError.value = null
  try {
    const res = await platformAuth.requestLocalPasswordChallenge()
    captchaId.value = res.captchaId
    captchaSvg.value = res.imageSvg
    captcha.value = ''
    clearFieldErrors()
  } catch (e) {
    const raw = e instanceof Error ? e.message : String(e)
    reportError(t('auth.captchaLoadFailed'), raw)
    console.warn('requestLocalPasswordChallenge failed', e)
  } finally {
    captchaLoading.value = false
  }
}

function validateLocalForm(): boolean {
  const next: typeof fieldErrors.value = {}
  if (!username.value.trim()) next.username = t('auth.page.usernameRequired')
  if (!password.value) next.password = t('auth.page.passwordRequired')
  if (!captcha.value.trim() || !captchaId.value) next.captcha = t('auth.page.captchaRequired')
  fieldErrors.value = next
  return Object.keys(next).length === 0
}

async function submitLocalLogin() {
  if (busy.value) return
  localError.value = null
  if (!validateLocalForm()) {
    focusFirstInvalid()
    return
  }
  submitting.value = true
  let restoreFocus = false
  try {
    await platformAuth.loginLocalPassword({
      username: username.value.trim(),
      password: password.value,
      captchaId: captchaId.value,
      captcha: captcha.value.trim()
    })
    password.value = ''
    captcha.value = ''
    clearFieldErrors()
  } catch (e) {
    const raw = e instanceof Error ? e.message : String(e)
    console.warn('local password login failed', e)
    // A rejected captcha is single-use: refresh first, then surface the error
    // (refreshCaptcha clears the message region).
    await refreshCaptcha()
    reportError(platformAuth.error, raw)
    restoreFocus = true
  } finally {
    submitting.value = false
    if (restoreFocus) {
      // Disabling the button mid-flight drops focus to <body>; move it to the
      // field that has to be retyped (never to the error text).
      await nextTick()
      // The captcha was consumed by the attempt: put the caret where the user
      // must type next. The error text itself never takes focus.
      captchaEl.value?.focus()
    }
  }
}

async function startProvider(providerId: string) {
  if (pendingProvider.value || busy.value) return
  pendingProvider.value = providerId
  localError.value = null
  try {
    await platformAuth.loginProvider(providerId)
  } catch (e) {
    const raw = e instanceof Error ? e.message : String(e)
    reportError(platformAuth.error, raw)
    console.warn('provider login failed', providerId, e)
  } finally {
    pendingProvider.value = null
  }
}

/** Expand a collapsible region and move focus into the revealed content. */
async function openLocalForm() {
  localFormOpen.value = true
  await nextTick()
  usernameEl.value?.focus()
  void refreshCaptcha()
}

async function toggleOtherMethods() {
  if (otherMethodsOpen.value) {
    closeOtherMethods()
    return
  }
  otherMethodsOpen.value = true
  await nextTick()
  firstOtherEl.value?.focus()
}

function closeOtherMethods() {
  otherMethodsOpen.value = false
  otherTriggerEl.value?.focus()
}

/** Back to the entry list: collapse the form and restore focus to its trigger. */
async function closeLocalForm() {
  if (!localFormOpen.value) return
  localFormOpen.value = false
  // The trigger is re-mounted by `v-if`: focus it after the re-render.
  await nextTick()
  localTriggerEl.value?.focus()
}

/** Esc collapses whatever is expanded and returns focus to its trigger. */
async function onEscape() {
  if (otherMethodsOpen.value) {
    closeOtherMethods()
    return
  }
  if (focusedLocalForm.value) await closeLocalForm()
}

/** Focus the entry the user is expected to start with (never steals focus). */
let autoFocused = false
async function maybeAutoFocus() {
  if (autoFocused) return
  await nextTick()
  const active = document.activeElement
  if (active && active !== document.body && active !== document.documentElement) {
    // The user already moved focus: leave it alone.
    autoFocused = true
    return
  }
  if (showLocalForm.value) {
    const input = usernameEl.value
    if (!input || input.disabled) return
    input.focus()
    if (document.activeElement === input) autoFocused = true
    return
  }
  const button = primaryButtonEl.value
  // The entry is disabled while the session/provider load runs: retry on the
  // next state change instead of silently doing nothing.
  if (!button || button.disabled) return
  button.focus()
  if (document.activeElement === button) autoFocused = true
}

// CSS height/opacity transition for the collapsible regions.
//
// Both hooks MUST resolve `done`: without it Vue keeps the leaving element in
// the DOM forever (height 0, inputs still tabbable) because the transition it
// waits on may never start when the two style writes collapse into one frame.
const transitionTimers = new WeakMap<HTMLElement, number>()

function clearTransitionTimer(node: HTMLElement) {
  const timer = transitionTimers.get(node)
  if (timer != null) {
    clearTimeout(timer)
    transitionTimers.delete(node)
  }
}

/** Resolve `done` on the height transitionend, with a safety net. */
function resolveWhenHeightSettles(node: HTMLElement, done: () => void) {
  const duration = parseFloat(getComputedStyle(node).transitionDuration) || 0
  if (duration === 0) {
    // No animation (e.g. prefers-reduced-motion): finish immediately.
    done()
    return
  }
  let finished = false
  const finish = () => {
    if (finished) return
    finished = true
    node.removeEventListener('transitionend', onEnd)
    clearTransitionTimer(node)
    done()
  }
  const onEnd = (event: TransitionEvent) => {
    if (event.target === node && event.propertyName === 'height') finish()
  }
  node.addEventListener('transitionend', onEnd)
  // Safety net: a missed transitionend must never strand the element.
  transitionTimers.set(node, window.setTimeout(finish, duration * 1000 + 120))
}

function onEnter(el: Element, done: () => void) {
  const node = el as HTMLElement
  clearTransitionTimer(node)
  node.style.height = '0px'
  node.style.opacity = '0'
  // Force a style flush so the start value is committed before the target one,
  // otherwise the browser merges both writes and no transition ever runs.
  void node.offsetHeight
  resolveWhenHeightSettles(node, done)
  node.style.height = `${node.scrollHeight}px`
  node.style.opacity = '1'
}

function onAfterEnter(el: Element) {
  const node = el as HTMLElement
  clearTransitionTimer(node)
  node.style.height = 'auto'
}

function onLeave(el: Element, done: () => void) {
  const node = el as HTMLElement
  clearTransitionTimer(node)
  node.style.height = `${node.scrollHeight}px`
  node.style.opacity = '1'
  void node.offsetHeight
  resolveWhenHeightSettles(node, done)
  node.style.height = '0px'
  node.style.opacity = '0'
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  void (async () => {
    providersLoading.value = true
    try {
      if (platformAuth.providers.length === 0) await platformAuth.loadAuthProviders()
    } finally {
      providersLoading.value = false
    }
    if (localIsPrimary.value) void refreshCaptcha()
    await maybeAutoFocus()
  })()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  if (cooldownTimer != null) {
    clearInterval(cooldownTimer)
    cooldownTimer = null
  }
})

function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  void onEscape()
}

// Focus once the entry is on screen *and* enabled (it is disabled while the
// session / provider load is in flight).
watch(
  [() => entries.value.length, () => busy.value, showLocalForm],
  ([count]) => {
    if (count === 0) return
    void maybeAutoFocus()
  },
  { flush: 'post' }
)

// A late provider list can flip which entry is primary (e.g. local_password).
watch(localIsPrimary, isPrimary => {
  if (isPrimary && !captchaSvg.value) void refreshCaptcha()
})
</script>

<template>
  <div class="h-full w-full overflow-y-auto bg-background">
    <div class="mx-auto flex min-h-full w-full max-w-sm flex-col justify-center px-6 py-12">
      <h1 class="mb-1 text-center text-xl font-semibold tracking-tight text-foreground">
        {{ t('auth.page.title') }}
      </h1>
      <p v-if="showSubtitle" class="mb-6 text-center text-xs text-muted">
        {{ t('auth.page.subtitle') }}
      </p>
      <p v-else class="mb-6" />

      <!-- Errors are announced politely; they never take focus. -->
      <div aria-live="polite" aria-atomic="true">
        <p
          v-if="displayError"
          class="mb-4 rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-xs leading-snug text-danger"
          role="status"
          data-testid="login-error"
        >
          {{ displayError }}
        </p>
      </div>

      <!-- Skeleton while the provider registry loads (avoids a blank flash). -->
      <div v-if="showSkeleton" class="flex flex-col gap-2" aria-hidden="true">
        <div
          v-for="n in 3"
          :key="n"
          class="h-11 w-full animate-pulse rounded-lg bg-muted/60"
          data-testid="login-skeleton"
        />
      </div>

      <p v-else-if="entries.length === 0" class="text-center text-xs text-muted">
        {{ t('auth.noProviders') }}
      </p>

      <template v-else>
        <!-- Primary entry: solid. `local_password` renders its form instead. -->
        <button
          v-if="primary && !localIsPrimary && !focusedLocalForm"
          ref="primaryButtonEl"
          type="button"
          class="flex w-full items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground transition hover:opacity-90 disabled:opacity-60"
          :disabled="pendingProvider !== null || busy"
          @click="startProvider(primary.id)"
        >
          <Loader2 v-if="pendingProvider === primary.id" class="h-4 w-4 animate-spin" />
          <component
            :is="providerIcon(primary.icon)"
            v-else-if="providerIcon(primary.icon)"
            class="h-4 w-4"
          />
          <span
            v-else
            class="flex h-4 w-4 items-center justify-center rounded-full bg-primary-foreground/20 text-[10px] font-semibold"
          >
            {{ providerInitial(primary) }}
          </span>
          <span>{{ withCooldown(buttonLabel(primary)) }}</span>
        </button>

        <!-- 只留表单 + 返回：主入口、「或」、其他次级入口在展开期间全部隐藏。 -->
        <button
          v-if="focusedLocalForm"
          type="button"
          class="mb-3 flex items-center gap-1 self-start rounded-md px-1 py-1 text-xs text-muted transition hover:text-foreground"
          data-testid="back-to-methods"
          @click="closeLocalForm"
        >
          <ArrowLeft class="h-3.5 w-3.5" />
          {{ t('auth.page.backToMethods') }}
        </button>

        <Transition
          name="expand"
          @enter="onEnter"
          @after-enter="onAfterEnter"
          @leave="onLeave"
        >
          <form
            v-if="showLocalForm"
            class="flex w-full flex-col gap-2"
            :inert="!showLocalForm || undefined"
            :aria-hidden="!showLocalForm || undefined"
            novalidate
            @submit.prevent="submitLocalLogin"
          >
            <div class="flex flex-col gap-1">
              <input
                ref="usernameEl"
                v-model="username"
                type="text"
                autocomplete="username"
                :placeholder="t('auth.username')"
                :aria-invalid="fieldErrors.username ? 'true' : undefined"
                :aria-describedby="fieldErrors.username ? 'login-username-error' : undefined"
                class="h-11 w-full rounded-lg border bg-background px-3 text-sm text-foreground outline-none focus:border-primary"
                :class="fieldErrors.username ? 'border-danger' : 'border-border'"
                @input="fieldErrors.username = undefined"
              />
              <p
                v-if="fieldErrors.username"
                id="login-username-error"
                class="px-1 text-[11px] leading-snug text-danger"
              >
                {{ fieldErrors.username }}
              </p>
            </div>

            <div class="flex flex-col gap-1">
              <div class="relative">
                <input
                  ref="passwordEl"
                  v-model="password"
                  :type="passwordVisible ? 'text' : 'password'"
                  autocomplete="current-password"
                  :placeholder="t('auth.password')"
                  :aria-invalid="fieldErrors.password ? 'true' : undefined"
                  :aria-describedby="fieldErrors.password ? 'login-password-error' : undefined"
                  class="h-11 w-full rounded-lg border bg-background px-3 pr-10 text-sm text-foreground outline-none focus:border-primary"
                  :class="fieldErrors.password ? 'border-danger' : 'border-border'"
                  @input="fieldErrors.password = undefined"
                />
                <button
                  type="button"
                  class="absolute right-1 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-md text-muted transition hover:text-foreground"
                  :aria-label="passwordVisible ? t('auth.page.hidePassword') : t('auth.page.showPassword')"
                  :aria-pressed="passwordVisible"
                  data-testid="toggle-password"
                  @click="passwordVisible = !passwordVisible"
                >
                  <EyeOff v-if="passwordVisible" class="h-4 w-4" />
                  <Eye v-else class="h-4 w-4" />
                </button>
              </div>
              <p
                v-if="fieldErrors.password"
                id="login-password-error"
                class="px-1 text-[11px] leading-snug text-danger"
              >
                {{ fieldErrors.password }}
              </p>
            </div>

            <!-- 验证码行：输入框 flex-1 + 图片固定宽 + 刷新按钮，三者同高同行。 -->
            <div class="flex flex-col gap-1">
              <div class="flex flex-nowrap items-center gap-2">
                <input
                  ref="captchaEl"
                  v-model="captcha"
                  type="text"
                  autocomplete="off"
                  :placeholder="t('auth.captcha')"
                  :aria-invalid="fieldErrors.captcha ? 'true' : undefined"
                  :aria-describedby="fieldErrors.captcha ? 'login-captcha-error' : undefined"
                  class="h-11 min-w-0 flex-1 rounded-lg border bg-background px-3 text-sm tracking-widest text-foreground outline-none focus:border-primary"
                  :class="fieldErrors.captcha ? 'border-danger' : 'border-border'"
                  @input="fieldErrors.captcha = undefined"
                />
                <div
                  class="captcha-frame flex h-11 w-[140px] shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border"
                  role="img"
                  :aria-label="t('auth.captcha')"
                  v-html="captchaSvg"
                />
                <button
                  type="button"
                  class="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg border border-border text-muted transition hover:text-foreground disabled:opacity-60"
                  :disabled="captchaLoading"
                  :title="t('auth.refreshCaptcha')"
                  :aria-label="t('auth.refreshCaptcha')"
                  @click="refreshCaptcha"
                >
                  <Loader2 v-if="captchaLoading" class="h-4 w-4 animate-spin" />
                  <RefreshCw v-else class="h-4 w-4" />
                </button>
              </div>
              <p
                v-if="fieldErrors.captcha"
                id="login-captcha-error"
                class="px-1 text-[11px] leading-snug text-danger"
              >
                {{ fieldErrors.captcha }}
              </p>
            </div>

            <button
              type="submit"
              class="mt-1 flex h-11 w-full items-center justify-center gap-2 rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:opacity-90 disabled:opacity-60"
              :disabled="submitting || busy"
            >
              <Loader2 v-if="submitting" class="h-4 w-4 animate-spin" />
              <span>{{
                withCooldown(
                  buttonLabel(
                    localProvider ?? { id: 'local_password', display_name: t('auth.password'), kind: 'local_password' }
                  )
                )
              }}</span>
            </button>
          </form>
        </Transition>

        <div v-if="showOr && !focusedLocalForm" class="my-4 flex items-center gap-3">
          <span class="h-px flex-1 bg-border" />
          <span class="text-[11px] uppercase tracking-wide text-muted">{{ t('auth.page.or') }}</span>
          <span class="h-px flex-1 bg-border" />
        </div>

        <!-- Secondary: outlined. `local_password` keeps its own row. -->
        <button
          v-if="localSecondary && !localFormOpen"
          ref="localTriggerEl"
          type="button"
          class="flex w-full items-center justify-center gap-2 rounded-lg border border-border px-4 py-2.5 text-sm font-medium text-foreground transition hover:bg-muted/40 disabled:opacity-60"
          :disabled="busy"
          data-testid="local-password-trigger"
          @click="openLocalForm"
        >
          <component
            :is="providerIcon(localSecondary.icon)"
            v-if="providerIcon(localSecondary.icon)"
            class="h-4 w-4"
          />
          <span
            v-else
            class="flex h-4 w-4 items-center justify-center rounded-full bg-muted text-[10px] font-semibold text-foreground"
          >
            {{ providerInitial(localSecondary) }}
          </span>
          {{ buttonLabel(localSecondary) }}
        </button>

        <button
          v-for="provider in groupOtherMethods || focusedLocalForm ? [] : ssoSecondaries"
          :key="provider.id"
          type="button"
          class="mt-2 flex w-full items-center justify-center gap-2 rounded-lg border border-border px-4 py-2.5 text-sm font-medium text-foreground transition hover:bg-muted/40 disabled:opacity-60"
          :disabled="pendingProvider !== null || busy"
          @click="startProvider(provider.id)"
        >
          <Loader2 v-if="pendingProvider === provider.id" class="h-4 w-4 animate-spin" />
          <component
            :is="providerIcon(provider.icon)"
            v-else-if="providerIcon(provider.icon)"
            class="h-4 w-4"
          />
          <span
            v-else
            class="flex h-4 w-4 items-center justify-center rounded-full bg-muted text-[10px] font-semibold text-foreground"
          >
            {{ providerInitial(provider) }}
          </span>
          {{ buttonLabel(provider) }}
        </button>

        <template v-if="groupOtherMethods && !focusedLocalForm">
          <button
            ref="otherTriggerEl"
            type="button"
            class="mt-2 flex w-full items-center justify-center gap-2 rounded-lg border border-border px-4 py-2.5 text-sm font-medium text-foreground transition hover:bg-muted/40"
            :aria-expanded="otherMethodsOpen"
            aria-controls="login-other-methods"
            data-testid="other-methods-trigger"
            @click="toggleOtherMethods"
          >
            <Ellipsis class="h-4 w-4" />
            {{ t('auth.page.otherMethods', { n: ssoSecondaries.length }) }}
          </button>
          <Transition
            name="expand"
            @enter="onEnter"
            @after-enter="onAfterEnter"
            @leave="onLeave"
          >
            <div
              v-if="otherMethodsOpen"
              id="login-other-methods"
              class="mt-2 flex flex-col gap-2"
              :inert="!otherMethodsOpen || undefined"
              :aria-hidden="!otherMethodsOpen || undefined"
            >
              <button
                v-for="(provider, index) in ssoSecondaries"
                :key="provider.id"
                :ref="el => { if (index === 0) firstOtherEl = el as HTMLButtonElement | null }"
                type="button"
                class="flex w-full items-center justify-center gap-2 rounded-lg border border-border px-4 py-2.5 text-sm font-medium text-foreground transition hover:bg-muted/40 disabled:opacity-60"
                :disabled="pendingProvider !== null || busy"
                @click="startProvider(provider.id)"
              >
                <Loader2 v-if="pendingProvider === provider.id" class="h-4 w-4 animate-spin" />
                <component
                  :is="providerIcon(provider.icon)"
                  v-else-if="providerIcon(provider.icon)"
                  class="h-4 w-4"
                />
                <span
                  v-else
                  class="flex h-4 w-4 items-center justify-center rounded-full bg-muted text-[10px] font-semibold text-foreground"
                >
                  {{ providerInitial(provider) }}
                </span>
                {{ buttonLabel(provider) }}
              </button>
            </div>
          </Transition>
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* Captcha image frame: the server SVG carries its own #f3f4f6 canvas plus a
   fixed 140x48 size. The frame is 140 wide and row-height tall, and the SVG is
   scaled to fit (viewBox kept, so the glyphs never distort); the frame paints
   the same canvas colour so the letterbox bars are invisible. */
.captcha-frame {
  background-color: #f3f4f6;
}
.captcha-frame :deep(svg) {
  display: block;
  width: 100%;
  height: 100%;
}

/* Height + opacity transition for the collapsible regions (no JS library). */
.expand-enter-active,
.expand-leave-active {
  overflow: hidden;
  transition: height 180ms ease, opacity 180ms ease;
}

.expand-enter-from,
.expand-leave-to {
  height: 0;
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .expand-enter-active,
  .expand-leave-active {
    transition: none;
  }
}
</style>
