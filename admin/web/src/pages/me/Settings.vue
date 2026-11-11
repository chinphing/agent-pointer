<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../../api'
import { session } from '../../store'
import { roleLabel } from '../../lib/roles'
import { when } from '../../lib/format'

const { t } = useI18n()

const displayName = computed(
  () => session.user?.displayName || session.user?.username || '—'
)
const initial = computed(() => displayName.value.trim().slice(0, 1).toUpperCase())
const roleNames = computed(() =>
  (session.roles || []).map((r: any) => {
    const key = typeof r === 'string' ? r : r.key
    const name = typeof r === 'string' ? '' : r.name
    return roleLabel(key, name, t)
  })
)
const isActive = computed(() => session.user?.status === 'active')

/**
 * 自助改密码。
 *
 * 后端改完会作废旧会话并发一个新 token 写回 Cookie，所以这里不用跳登录页，
 * 只要把表单清空、提示一句「其他设备上的登录已失效」。
 */
/** 展示态 / 编辑态分离：默认只显示「已设置」，点「修改密码」才展开表单。 */
const editing = ref(false)
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const saving = ref(false)
const error = ref('')
const notice = ref('')

const canSubmit = computed(
  () =>
    !saving.value &&
    oldPassword.value.length > 0 &&
    newPassword.value.length > 0 &&
    confirmPassword.value.length > 0
)

function cancelEdit() {
  editing.value = false
  oldPassword.value = ''
  newPassword.value = ''
  confirmPassword.value = ''
  error.value = ''
}

async function submit() {
  error.value = ''
  notice.value = ''

  // 前端先挡一道，少一次往返；后端同样会校验，不能只靠这里。
  if (newPassword.value.trim().length < 8) {
    error.value = t('me.passwordTooShort')
    return
  }
  if (newPassword.value !== confirmPassword.value) {
    error.value = t('me.passwordMismatch')
    return
  }
  if (newPassword.value === oldPassword.value) {
    error.value = t('me.passwordSameAsOld')
    return
  }

  saving.value = true
  try {
    await api.post('/api/admin/me/password', {
      old_password: oldPassword.value,
      new_password: newPassword.value,
    })
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    editing.value = false
    notice.value = t('me.passwordChanged')
  } catch (e) {
    error.value = errText(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="me-page">
    <section class="me-card">
      <div class="me-card-head">
        <div class="me-identity">
          <div class="me-avatar">{{ initial }}</div>
          <div style="min-width: 0">
            <div class="me-identity-name">{{ displayName }}</div>
            <div class="me-identity-meta">{{ session.user?.username }}</div>
          </div>
        </div>
        <span class="tag" :class="isActive ? 'ok' : 'plain'">
          {{ isActive ? t('common.enabled') : t('common.disabled') }}
        </span>
      </div>

      <div class="me-kv">
        <div class="me-kv-row">
          <span class="me-kv-key">{{ t('me.username') }}</span>
          <span class="me-kv-val mono">{{ session.user?.username }}</span>
        </div>
        <div class="me-kv-row">
          <span class="me-kv-key">{{ t('me.displayName') }}</span>
          <span class="me-kv-val">{{ session.user?.displayName || '—' }}</span>
        </div>
        <div class="me-kv-row">
          <span class="me-kv-key">{{ t('me.roles') }}</span>
          <span class="me-kv-val">{{ roleNames.join(' / ') || t('app.noRole') }}</span>
        </div>
        <div class="me-kv-row">
          <span class="me-kv-key">{{ t('me.lastLogin') }}</span>
          <span class="me-kv-val muted">
            {{ session.user?.lastLoginAt ? when(session.user.lastLoginAt) : t('me.neverLoggedIn') }}
          </span>
        </div>
      </div>
    </section>

    <section class="me-card">
      <div class="me-card-head">
        <div>
          <div class="me-card-title">{{ t('me.changePassword') }}</div>
          <div class="me-card-sub">{{ t('me.changePasswordHint') }}</div>
        </div>
      </div>

      <!-- 展示态 -->
      <div v-if="!editing" class="row" style="align-items: center; gap: 10px">
        <span class="pw-state">{{ t('me.passwordSet') }}</span>
        <button class="btn" type="button" @click="editing = true">
          {{ t('me.changePassword') }}
        </button>
      </div>

      <!-- 编辑态 -->
      <form v-else class="pw-form" @submit.prevent="submit">
        <label class="pw-field">
          <span class="pw-label">{{ t('me.currentPassword') }}</span>
          <input
            v-model="oldPassword"
            type="password"
            autocomplete="current-password"
            :placeholder="t('me.currentPassword')"
          />
        </label>

        <label class="pw-field">
          <span class="pw-label">{{ t('me.newPassword') }}</span>
          <input
            v-model="newPassword"
            type="password"
            autocomplete="new-password"
            :placeholder="t('me.newPasswordMin')"
          />
        </label>

        <label class="pw-field">
          <span class="pw-label">{{ t('me.confirmPassword') }}</span>
          <input
            v-model="confirmPassword"
            type="password"
            autocomplete="new-password"
            :placeholder="t('me.confirmPassword')"
          />
        </label>

        <div v-if="error" class="err" style="margin: 0">{{ error }}</div>
        <div v-if="notice" class="okmsg" style="margin: 0">{{ notice }}</div>

        <div class="row" style="gap: 10px">
          <button type="submit" class="btn primary" :disabled="!canSubmit">
            {{ saving ? t('common.loading') : t('common.save') }}
          </button>
          <button type="button" class="btn" :disabled="saving" @click="cancelEdit">
            {{ t('common.cancel') }}
          </button>
        </div>
      </form>
    </section>
  </div>
</template>

<style scoped>
.pw-form { display: flex; flex-direction: column; gap: 14px; max-width: 420px; }
.pw-state { color: var(--muted); font-size: 13px; }
.pw-field { display: flex; flex-direction: column; gap: 6px; }
.pw-label { font-size: 12px; color: var(--muted); }
.pw-field input { height: 34px; }
</style>
