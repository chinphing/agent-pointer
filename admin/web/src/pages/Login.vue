<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { login, session } from '../store'
import { landingFor } from '../routes'
import { api, errText } from '../api'
import { LOGIN_METHODS, type LoginMethod } from '@ext'

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
 * 账号密码之外的登录方式由扩展提供（开源版为空）。
 *
 * 开源仓不知道「短信」这回事：它只渲染扩展给的 tab，加载扩展给的组件。
 * 能力探测也由扩展自己做，没开通的方式不会出现在 tab 里。
 */
const methods = ref<LoginMethod[]>([])
const active = ref('password')

const activeMethod = computed(() => methods.value.find((m) => m.key === active.value) ?? null)
const activeComponent = computed(() =>
  activeMethod.value ? defineAsyncComponent(activeMethod.value.component) : null
)

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

onMounted(async () => {
  void refreshCaptcha()

  const probed = await Promise.all(
    LOGIN_METHODS.map(async (m) => (((await m.available?.()) ?? true) ? m : null))
  )
  methods.value = probed.filter((m): m is LoginMethod => m !== null)
  // 有扩展方式时默认用它：私有部署装短信就是为了少输密码。
  if (methods.value.length) active.value = methods.value[0].key
})

onUnmounted(() => {
  /* 无需清理：扩展组件自己管自己的定时器 */
})

/** 密码与扩展方式互切。切换时清掉上一次的错误，否则会带着旧报错进新表单。 */
function switchMode() {
  active.value = active.value === 'password' ? (methods.value[0]?.key ?? 'password') : 'password'
  error.value = ''
}

/** 扩展方式登录成功：和密码登录走同一套落地逻辑。 */
function onMethodSuccess() {
  router.replace(nextPath || landingFor(session.permissions))
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
      <div class="head">
        <h1>{{ t('login.title') }}</h1>
        <!-- 只有一种扩展方式时保持右上角的切换链接；多种才用 tab 行 -->
        <button v-if="methods.length === 1" type="button" class="link" @click="switchMode">
          {{ active === 'password' ? t(methods[0].labelKey) : t('login.passwordTab') }}
        </button>
      </div>

      <div v-if="methods.length > 1" class="tabs">
        <button
          type="button"
          :class="{ active: active === 'password' }"
          @click="active = 'password'"
        >
          {{ t('login.passwordTab') }}
        </button>
        <button
          v-for="m in methods"
          :key="m.key"
          type="button"
          :class="{ active: active === m.key }"
          @click="active = m.key"
        >
          {{ t(m.labelKey) }}
        </button>
      </div>

      <p v-if="active === 'password'" class="muted small">{{ t('login.subtitle') }}</p>

      <div v-if="error" class="err" style="margin-top: 14px">{{ error }}</div>

      <component :is="activeComponent" v-if="activeComponent" @success="onMethodSuccess" />

      <form v-else @submit.prevent="submitPassword">
        <label for="u">{{ t('login.account') }}</label>
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
h1 {
  font-size: 18px;
  margin-bottom: 6px;
}
p {
  margin: 0;
}
.head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}
.link {
  border: 0;
  background: none;
  padding: 0;
  font-size: 12px;
  color: var(--accent);
  cursor: pointer;
}
.link:hover {
  text-decoration: underline;
}
.tabs {
  display: flex;
  gap: 6px;
  margin: 14px 0 0;
}
.tabs button {
  border: 1px solid var(--line);
  background: #fff;
  border-radius: 8px;
  padding: 6px 12px;
  font-size: 13px;
  cursor: pointer;
}
.tabs button.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
</style>
