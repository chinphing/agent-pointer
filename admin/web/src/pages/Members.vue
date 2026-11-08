<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { can } from '../store'
import { addableRoles } from '../lib/roles'
import { when } from '../lib/format'

const items = ref<any[]>([])
const roles = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const notice = ref('')

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
    notice.value = '账号已创建'
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function toggleStatus(user: any) {
  const next = user.status === 'active' ? 'disabled' : 'active'
  try {
    await api.post(`/api/admin/users/${user.id}/status`, { status: next })
    notice.value = next === 'active' ? '已启用' : '已停用（该账号所有会话已踢下线）'
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function resetPassword(user: any) {
  const pw = prompt(`为「${user.displayName || user.username}」设置新密码（至少 8 位）`)
  if (!pw) return
  try {
    await api.post(`/api/admin/users/${user.id}/password`, { password: pw })
    notice.value = '密码已重置，该账号需要重新登录'
  } catch (e) {
    error.value = errText(e)
  }
}

async function saveRoles(user: any, selected: string[]) {
  try {
    await api.post(`/api/admin/users/${user.id}/roles`, { roles: selected })
    notice.value = '角色已更新'
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

onMounted(load)
</script>

<template>
  <div class="spread">
    <div>
      <h1>成员</h1>
      <p class="muted small" style="margin: 4px 0 0">账号只能由管理员创建，不开放自注册。</p>
    </div>
    <div class="row" v-if="can('user:write')">
      <button @click="showImport = true; importResult = null">批量导入</button>
      <button class="primary" @click="showCreate = true">新建账号</button>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>

  <div class="panel" style="margin-top: 16px">
    <div v-if="loading" class="empty">加载中…</div>
    <div v-else-if="!items.length" class="empty">还没有成员</div>
    <table v-else>
      <thead>
        <tr>
          <th style="width: 150px">账号</th>
          <th style="width: 130px">姓名</th>
          <th style="width: 84px">状态</th>
          <th>角色</th>
          <th style="width: 150px">最近登录</th>
          <th style="width: 176px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="user in items" :key="user.id">
          <td class="mono">{{ user.username }}</td>
          <td>{{ user.displayName || '—' }}</td>
          <td>
            <span class="tag" :class="user.status === 'active' ? 'ok' : 'plain'">
              {{ user.status === 'active' ? '启用' : '停用' }}
            </span>
          </td>
          <td>
            <div class="role-cell">
              <span
                v-for="key in user.roles"
                :key="key"
                class="tag"
                :style="{ cursor: can('user:write') ? 'pointer' : 'default' }"
                :title="can('user:write') ? '点击移除该角色' : ''"
                @click="can('user:write') && toggleRole(user, key)"
              >{{ key }}</span>
              <span v-if="!user.roles.length" class="muted small">无角色</span>
              <select
                v-if="can('user:write') && addableFor(user).length"
                class="tiny"
                title="添加角色"
                @change="onAddRole(user, $event)"
              >
                <option value="">+ 角色</option>
                <option v-for="r in addableFor(user)" :key="r.key" :value="r.key">
                  {{ r.name || r.key }}
                </option>
              </select>
            </div>
          </td>
          <td class="small muted">{{ user.lastLoginAt ? when(user.lastLoginAt) : '从未登录' }}</td>
          <td class="actions">
            <div class="actions-row">
              <button v-if="can('user:write')" class="tiny" @click="resetPassword(user)">重置密码</button>
              <button
                v-if="can('user:write')"
                class="tiny"
                :class="{ danger: user.status === 'active' }"
                @click="toggleStatus(user)"
              >
                {{ user.status === 'active' ? '停用' : '启用' }}
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <div v-if="showCreate" class="modal-mask" @click.self="showCreate = false">
    <div class="modal">
      <h2>新建账号</h2>
      <label>账号</label>
      <input v-model="form.username" placeholder="登录名，不含空格" />
      <label>姓名</label>
      <input v-model="form.displayName" placeholder="显示名" />
      <label>初始密码</label>
      <input v-model="form.password" type="password" placeholder="至少 8 位" />
      <label>角色</label>
      <div class="row wrap">
        <label v-for="key in roleKeys()" :key="key" class="row" style="margin: 0; gap: 6px">
          <input
            type="checkbox"
            style="width: auto"
            :value="key"
            v-model="form.roles"
          />
          <span class="small">{{ key }}</span>
        </label>
      </div>
      <div class="modal-actions">
        <button @click="showCreate = false">取消</button>
        <button class="primary" :disabled="busy" @click="createUser">创建</button>
      </div>
    </div>
  </div>

  <div v-if="showImport" class="modal-mask" @click.self="showImport = false">
    <div class="modal">
      <h2>批量导入</h2>
      <p class="muted small">每行 <span class="mono">账号,姓名,初始密码[,角色]</span>，首行可为表头。坏行会跳过并列出原因。</p>
      <textarea v-model="csv" rows="8" style="margin-top: 10px; font-family: ui-monospace, monospace"></textarea>
      <div v-if="importResult" class="okmsg" style="margin-top: 12px">
        成功 {{ importResult.created }} 个，跳过 {{ importResult.skipped.length }} 行
        <div v-for="row in importResult.skipped" :key="row.line" class="small">
          第 {{ row.line }} 行：{{ row.reason }}
        </div>
      </div>
      <div class="modal-actions">
        <button @click="showImport = false">关闭</button>
        <button class="primary" :disabled="busy" @click="runImport">导入</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.role-cell { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
</style>
