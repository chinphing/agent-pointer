<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useNotice } from '../lib/notice'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { can } from '../store'
import { addableRoles, roleLabel } from '../lib/roles'
import { when } from '../lib/format'

const { t } = useI18n()
const route = useRoute()

/**
 * 角色名统一走 roleLabel：内置角色翻译成当前语言，自定义角色用管理员起的名字。
 * 之前这里直接渲染 key（super_admin），而别处渲染 name（超级管理员），
 * 于是同一个角色在两个地方一个中文一个英文。
 */
function label(key: string) {
  const r = roles.value.find((x) => x.key === key)
  return roleLabel(key, r?.name, t)
}

const items = ref<any[]>([])
/** 顶栏搜索的关键词（?q=）：账号 / 姓名模糊匹配。 */
const keyword = ref(typeof route.query.q === 'string' ? route.query.q : '')
const shown = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return items.value
  return items.value.filter(
    (u) =>
      String(u.username || '').toLowerCase().includes(q) ||
      String(u.displayName || '').toLowerCase().includes(q)
  )
})
const roles = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()

const showCreate = ref(false)
const form = ref({ username: '', displayName: '', password: '', roles: ['member'] })
const busy = ref(false)

const showImport = ref(false)
const csv = ref('username,display_name,password,role\nalice,爱丽丝,Init@12345,member')
const importResult = ref<any>(null)

const roleKeys = () => roles.value.map((r) => r.key)

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [users, roleList] = await Promise.all([
      api.get('/api/admin/users'),
      api.get('/api/admin/roles'),
    ])
    items.value = users.items
    roles.value = roleList.items
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

async function createUser() {
  busy.value = true
  error.value = ''
  try {
    await api.post('/api/admin/users', {
      username: form.value.username.trim(),
      displayName: form.value.displayName.trim(),
      password: form.value.password,
      roles: form.value.roles,
    })
    showCreate.value = false
    form.value = { username: '', displayName: '', password: '', roles: ['member'] }
    flashNotice(t('members.created'))
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function grantCredit(user: any) {
  const raw = prompt(
    t('members.grantPrompt', { name: user.displayName || user.username }),
    '10'
  )
  if (!raw) return
  const amount = Number(raw)
  if (!Number.isFinite(amount) || amount <= 0) {
    error.value = t('members.grantInvalid')
    return
  }
  try {
    const res = await api.post(`/api/admin/users/${user.id}/wallet/grant`, {
      amountYuan: amount,
    })
    flashNotice(t('members.grantDone', { amount, balance: res.balanceYuan }))
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function toggleStatus(user: any) {
  const next = user.status === 'active' ? 'disabled' : 'active'
  try {
    await api.post(`/api/admin/users/${user.id}/status`, { status: next })
    notice.value = next === 'active' ? t('members.enabledNotice') : t('members.disabledNotice')
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function resetPassword(user: any) {
  const pw = prompt(
    t('members.resetPrompt', { name: user.displayName || user.username })
  )
  if (!pw) return
  try {
    await api.post(`/api/admin/users/${user.id}/password`, { password: pw })
    flashNotice(t('members.resetDone'))
  } catch (e) {
    error.value = errText(e)
  }
}

async function saveRoles(user: any, selected: string[]) {
  try {
    await api.post(`/api/admin/users/${user.id}/roles`, { roles: selected })
    flashNotice(t('members.rolesUpdated'))
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

function toggleRole(user: any, key: string) {
  const current: string[] = [...(user.roles || [])]
  const idx = current.indexOf(key)
  if (idx >= 0) current.splice(idx, 1)
  else current.push(key)
  saveRoles(user, current)
}

/** 「+ 角色」下拉里可选的项：还没分给这个人的角色。 */
function addableFor(user: any) {
  return addableRoles(roles.value, user.roles || [])
}

function onAddRole(user: any, ev: Event) {
  const el = ev.target as HTMLSelectElement
  const key = el.value
  el.value = ''
  if (key) toggleRole(user, key)
}

async function runImport() {
  busy.value = true
  error.value = ''
  try {
    importResult.value = await api.post('/api/admin/users/import', { csv: csv.value })
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

watch(
  () => route.query.q,
  (q) => {
    keyword.value = typeof q === 'string' ? q : ''
  }
)

onMounted(load)
</script>

<template>
  <div class="spread">
    <div>
      <p class="muted small" style="margin: 0">{{ t('members.subtitle') }}</p>
    </div>
    <div class="row" v-if="can('user:write')">
      <button @click="showImport = true; importResult = null">{{ t('members.import') }}</button>
      <button class="primary" @click="showCreate = true">{{ t('members.create') }}</button>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>

  <div class="panel" style="margin-top: 16px">
    <div v-if="loading" class="empty">{{ t('common.loading') }}</div>
    <div v-else-if="!items.length" class="empty">{{ t('members.empty') }}</div>
    <div v-else-if="!shown.length" class="empty">{{ t('members.noMatch') }}</div>
    <table v-else>
      <thead>
        <tr>
          <th style="width: 150px">{{ t('members.account') }}</th>
          <th style="width: 130px">{{ t('members.name') }}</th>
          <th style="width: 84px">{{ t('members.status') }}</th>
          <th style="width: 110px">{{ t('members.balance') }}</th>
          <th>{{ t('members.roles') }}</th>
          <th style="width: 150px">{{ t('members.lastLogin') }}</th>
          <th style="width: 176px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="user in shown" :key="user.id">
          <td class="mono">{{ user.username }}</td>
          <td>{{ user.displayName || '—' }}</td>
          <td>
            <span class="tag" :class="user.status === 'active' ? 'ok' : 'plain'">
              {{ user.status === 'active' ? t('common.enabled') : t('common.disabled') }}
            </span>
          </td>
          <td>
            <div class="role-cell">
              <span
                v-for="key in user.roles"
                :key="key"
                class="tag"
                :style="{ cursor: can('user:write') ? 'pointer' : 'default' }"
                :title="can('user:write') ? t('members.removeRoleHint') : ''"
                @click="can('user:write') && toggleRole(user, key)"
              >{{ label(key) }}</span>
              <span v-if="!user.roles.length" class="muted small">{{ t('roles.noRole') }}</span>
              <select
                v-if="can('user:write') && addableFor(user).length"
                class="tiny"
                :title="t('members.addRoleHint')"
                @change="onAddRole(user, $event)"
              >
                <option value="">{{ t('roles.add') }}</option>
                <option v-for="r in addableFor(user)" :key="r.key" :value="r.key">
                  {{ label(r.key) }}
                </option>
              </select>
            </div>
          </td>
          <td class="mono">
            <span v-if="user.balanceYuan !== null && user.balanceYuan !== undefined">
              ¥{{ user.balanceYuan }}
            </span>
            <span v-else class="muted small">{{ t('members.balanceUnopened') }}</span>
          </td>
          <td class="small muted">
            {{ user.lastLoginAt ? when(user.lastLoginAt) : t('members.neverLoggedIn') }}
          </td>
          <td class="actions">
            <div class="actions-row">
              <button v-if="can('usage:write')" class="tiny" @click="grantCredit(user)">
                {{ t('members.grant') }}
              </button>
              <button v-if="can('user:write')" class="tiny" @click="resetPassword(user)">
                {{ t('members.resetPassword') }}
              </button>
              <button
                v-if="can('user:write')"
                class="tiny"
                :class="{ danger: user.status === 'active' }"
                @click="toggleStatus(user)"
              >
                {{ user.status === 'active' ? t('members.disable') : t('members.enable') }}
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <div v-if="showCreate" class="modal-mask" @click.self="showCreate = false">
    <div class="modal">
      <h2>{{ t('members.create') }}</h2>
      <label>{{ t('members.account') }}</label>
      <input v-model="form.username" :placeholder="t('members.usernamePlaceholder')" />
      <label>{{ t('members.name') }}</label>
      <input v-model="form.displayName" :placeholder="t('members.displayNamePlaceholder')" />
      <label>{{ t('members.initialPassword') }}</label>
      <input v-model="form.password" type="password" :placeholder="t('members.passwordPlaceholder')" />
      <label>{{ t('members.roles') }}</label>
      <div class="row wrap">
        <label v-for="key in roleKeys()" :key="key" class="row" style="margin: 0; gap: 6px">
          <input
            type="checkbox"
            style="width: auto"
            :value="key"
            v-model="form.roles"
          />
          <span class="small">{{ label(key) }}</span>
        </label>
      </div>
      <div class="modal-actions">
        <button @click="showCreate = false">{{ t('common.cancel') }}</button>
        <button class="primary" :disabled="busy" @click="createUser">
          {{ t('members.createSubmit') }}
        </button>
      </div>
    </div>
  </div>

  <div v-if="showImport" class="modal-mask" @click.self="showImport = false">
    <div class="modal">
      <h2>{{ t('members.import') }}</h2>
      <p class="muted small">{{ t('members.importHint') }}</p>
      <textarea v-model="csv" rows="8" style="margin-top: 10px; font-family: ui-monospace, monospace"></textarea>
      <div v-if="importResult" class="okmsg" style="margin-top: 12px">
        {{ t('members.importResult', { created: importResult.created, skipped: importResult.skipped.length }) }}
        <div v-for="row in importResult.skipped" :key="row.line" class="small">
          {{ t('members.importLine', { line: row.line, reason: row.reason }) }}
        </div>
      </div>
      <div class="modal-actions">
        <button @click="showImport = false">{{ t('common.close') }}</button>
        <button class="primary" :disabled="busy" @click="runImport">
          {{ t('members.importSubmit') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.role-cell { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
</style>
