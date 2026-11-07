<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { login } from '../store'
import { errText } from '../api'

const router = useRouter()
const username = ref('')
const password = ref('')
const error = ref('')
const busy = ref(false)

async function submit() {
  error.value = ''
  busy.value = true
  try {
    await login(username.value.trim(), password.value)
    router.replace('/overview')
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="wrap">
    <form class="card" @submit.prevent="submit">
      <h1>登录 Pointer 控制面</h1>
      <p class="muted small">账号由管理员统一创建，不开放自注册。</p>

      <div v-if="error" class="err" style="margin-top: 14px">{{ error }}</div>

      <label for="u">账号</label>
      <input id="u" v-model="username" autocomplete="username" autofocus required />

      <label for="p">密码</label>
      <input id="p" v-model="password" type="password" autocomplete="current-password" required />

      <button class="primary" style="width: 100%; margin-top: 20px" :disabled="busy">
        {{ busy ? '登录中…' : '登录' }}
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
