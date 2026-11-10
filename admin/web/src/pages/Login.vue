<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { login, loginWithSms, session } from '../store'
import { landingFor } from '../routes'
import { api, errText } from '../api'

const router = useRouter()
const route = useRoute()
const { t } = useI18n()

/**
 * 登录后要回到的页面（守卫放进来的 `?next=`）。
 *
 * 只接受站内绝对路径：`//evil.com` 是协议相对的绝对 URL，`javascript:` 之类
 * 也不是以 `/` 开头，都要挡掉，否则登录页就成了开放重定向。
 */
function safeNext(raw: unknown): string {
  if (typeof raw !== 'string') return ''
  const value = raw.trim()
  if (!value.startsWith('/') || value.startsWith('//')) return ''
  return value
}

const nextPath = safeNext(route.query.next)
const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaId = ref('')
const captchaImage = ref('')
const captchaRequired = ref(true)
const error = ref('')
const busy = ref(false)

/**
 * 短信登录是**私有部署才有的能力**，开源版的后端根本不注册那组路由。
 * 所以入口不能写死，得先探测：探测不到就不显示，避免出现点了必报错的 tab。
 */
const smsEnabled = ref(false)
const mode = ref<'password' | 'sms'>('password')
const phone = ref('')
const smsCode = ref('')
const countdown = ref(0)
let timer: ReturnType<typeof setInterval> | null = null

async function probeSms() {
  try {
    const r = await fetch('/api/auth/sms/enabled', { credentials: 'same-origin' })
    if (!r.ok) return
    const data = (await r.json()) as { enabled?: boolean }
    if (data.enabled) {
      smsEnabled.value = true
      mode.value = 'sms'
    }
  } catch {
    // 探测失败就当没有短信登录：宁可不显示，也不显示一个点了会报错的入口。
  }
}

/** 拉一张新验证码。每次登录失败都要换，旧的已经被消费掉了。 */
async function refreshCaptcha() {
  captcha.value = ''
  try {
    const r = await api.get('/api/admin/auth/captcha')
    captchaRequired.value = r.required !== false
    captchaId.value = r.id || ''
    captchaImage.value = r.image || ''
  } catch (e) {
    error.value = errText(e)
  }
}

onMounted(() => {
  void probeSms()
  void refreshCaptcha()
})

onUnmounted(() => {
  if (timer) clearInterval(timer)
})

function startCountdown() {
  countdown.value = 60
  if (timer) clearInterval(timer)
  timer = setInterval(() => {
    countdown.value -= 1
    if (countdown.value <= 0 && timer) {
      clearInterval(timer)
      timer = null
    }
  }, 1000)
}

async function sendCode() {
  error.value = ''
  busy.value = true
  try {
    await api.post('/api/auth/sms/send', { phone: phone.value.trim(), scene: 'sign_in' })
    startCountdown()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function submitSms() {
  error.value = ''
  busy.value = true
  try {
    await loginWithSms(phone.value.trim(), smsCode.value.trim())
    router.replace(nextPath || landingFor(session.permissions))
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function submitPassword() {
  error.value = ''
  busy.value = true
  try {
    await login(username.value.trim(), password.value, captchaId.value, captcha.value.trim())
    // 有 ?next= 就回原本要去的页面（客户端「余额」直接打开 /me/billing 的场景），
    // 没有才按权限算落地页：管理员进概览，普通用户进 /me。
    router.replace(nextPath || landingFor(session.permissions))
  } catch (e) {
    error.value = errText(e)
    await refreshCaptcha()
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="wrap">
    <div class="card">
      <h1>{{ t('login.title') }}</h1>
      <p class="muted small">{{ mode === 'sms' ? t('login.smsHint') : t('login.subtitle') }}</p>

      <div v-if="error" class="err" style="margin-top: 14px">{{ error }}</div>

      <form v-if="mode === 'sms'" @submit.prevent="submitSms">
        <div class="label-row">
          <label for="phone">{{ t('login.phone') }}</label>
          <button type="button" class="link" @click="mode = 'password'; error = ''">
            {{ t('login.passwordTab') }}
          </button>
        </div>
        <input
          id="phone"
          v-model="phone"
          type="tel"
          inputmode="numeric"
          autocomplete="tel"
          autofocus
          required
        />

        <label for="code">{{ t('login.smsCode') }}</label>
        <div class="row" style="gap: 8px; align-items: center">
          <input
            id="code"
            v-model="smsCode"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="6"
            required
            style="flex: 1"
          />
          <button
            type="button"
            class="ghost"
            :disabled="busy || countdown > 0 || !phone.trim()"
            @click="sendCode"
          >
            {{ countdown > 0 ? `${countdown}s` : t('login.sendCode') }}
          </button>
        </div>

        <button class="primary" style="width: 100%; margin-top: 20px" :disabled="busy">
          {{ busy ? t('login.submitting') : t('login.smsSubmit') }}
        </button>
      </form>

      <form v-else @submit.prevent="submitPassword">
        <div class="label-row">
          <label for="u">{{ t('login.account') }}</label>
          <button
            v-if="smsEnabled"
            type="button"
            class="link"
            @click="mode = 'sms'; error = ''"
          >
            {{ t('login.smsTab') }}
          </button>
        </div>
        <input id="u" v-model="username" autocomplete="username" autofocus required />

        <label for="p">{{ t('login.password') }}</label>
        <input
          id="p"
          v-model="password"
          type="password"
          autocomplete="current-password"
          required
        />

        <template v-if="captchaRequired">
          <label for="c">{{ t('login.captcha') }}</label>
          <div class="row" style="gap: 8px; align-items: center">
            <input
              id="c"
              v-model="captcha"
              autocomplete="off"
              maxlength="4"
              required
              style="flex: 1"
            />
            <img
              v-if="captchaImage"
              :src="captchaImage"
              :alt="t('login.captcha')"
              :title="t('login.refreshCaptcha')"
              style="height: 40px; cursor: pointer; border-radius: 8px"
              @click="refreshCaptcha"
            />
          </div>
        </template>

        <button class="primary" style="width: 100%; margin-top: 20px" :disabled="busy">
          {{ busy ? t('login.submitting') : t('login.submit') }}
        </button>
      </form>
    </div>
  </div>
</template>

<style scoped>
.wrap {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}
.card {
  width: 380px;
  background: #fff;
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 30px 32px;
}
h1 { font-size: 18px; margin-bottom: 6px; }
p { margin: 0; }
.label-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}
.link {
  border: 0;
  background: none;
  padding: 0;
  font-size: 12px;
  color: var(--accent);
  cursor: pointer;
}
.link:hover { text-decoration: underline; }
.ghost {
  flex-shrink: 0;
  border: 1px solid var(--line);
  background: #fff;
  border-radius: 8px;
  padding: 0 12px;
  height: 40px;
  font-size: 13px;
  cursor: pointer;
}
.ghost:disabled { opacity: 0.45; cursor: not-allowed; }
</style>
