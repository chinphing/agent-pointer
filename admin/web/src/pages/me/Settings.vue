<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
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
          <span class="me-kv-val muted">{{ session.user?.lastLoginAt ? when(session.user.lastLoginAt) : t('me.neverLoggedIn') }}</span>
        </div>
      </div>

      <p class="muted small" style="margin: 18px 0 0">{{ t('me.passwordHint') }}</p>
    </section>
  </div>
</template>
