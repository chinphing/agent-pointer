<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { login } from '../store'
import { api, errText } from '../api'

const router = useRouter()
const { t } = useI18n()
const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaId = ref('')
const captchaImage = ref('')
const captchaRequired = ref(true)
const error = ref('')
const busy = ref(false)

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

onMounted(refreshCaptcha)

async function submit() {
  error.value = ''
  busy.value = true
  try {
    await login(username.value.trim(), password.value, captchaId.value, captcha.value.trim())
    router.replace('/overview')
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
    <form class="card" @submit.prevent="submit">
      <h1>{{ t('login.title') }}</h1>
      <p class="muted small">{{ t('login.subtitle') }}</p>

      <div v-if="error" class="err" style="margin-top: 14px">{{ error }}</div>

      <label for="u">{{ t('login.account') }}</label>
      <input id="u" v-model="username" autocomplete="username" autofocus required />

      <label for="p">{{ t('login.password') }}</label>
      <input id="p" v-model="password" type="password" autocomplete="current-password" required />

      <template v-if="captchaRequired">
      <label for="c">{{ t('login.captcha') }}</label>
      <div class="row" style="gap: 8px; align-items: center">
        <input id="c" v-model="captcha" autocomplete="off" maxlength="4" required style="flex: 1" />
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
</style>
