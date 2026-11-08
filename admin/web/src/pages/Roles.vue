<script setup lang="ts">
/**
 * 角色与权限（设计图 04）。
 *
 * 内置角色是代码常量，权限随版本走——只读展示，改了会被下次升级覆盖。
 * 自定义角色存在 roles / role_permissions，可增删改；权限判定只读后者，
 * 所以建完立刻生效。
 */
import { computed, onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { can } from '../store'

const MODULE_LABEL: Record<string, string> = {
  user: '成员',
  role: '角色',
  provider: '模型与密钥',
  model: '模型',
  skill: '技能',
  plugin: '插件',
  mcp: 'MCP',
  usage: '用量与配额',
  audit: '审计日志',
  system: '系统',
}
const ACTION_LABEL: Record<string, string> = {
  read: '查看',
  write: '新建修改',
  publish: '发布',
  delete: '删除',
  reveal: '取明文',
}
const ACTION_ORDER = ['read', 'write', 'publish', 'delete', 'reveal']

const roles = ref<any[]>([])
const allPermissions = ref<string[]>([])
const loading = ref(true)
const error = ref('')
const notice = ref('')
const busy = ref(false)

// 编辑弹窗
const editing = ref<any | null>(null)
const form = ref({ key: '', name: '', permissions: [] as string[] })

const modules = computed(() => {
  const set = new Set<string>()
  for (const p of allPermissions.value) set.add(p.split(':')[0])
  return [...set].sort((a, b) => {
    const ai = Object.keys(MODULE_LABEL).indexOf(a)
    const bi = Object.keys(MODULE_LABEL).indexOf(b)
    return (ai < 0 ? 99 : ai) - (bi < 0 ? 99 : bi)
  })
})

function moduleLabel(m: string) {
  return MODULE_LABEL[m] || m
}

/** 该模块下有哪些动作（不是每个模块都有 5 个动作）。 */
function actionsOf(m: string) {
  const have = new Set(
    allPermissions.value.filter((p) => p.startsWith(`${m}:`)).map((p) => p.split(':')[1])
  )
  return ACTION_ORDER.filter((a) => have.has(a))
}

function has(perm: string) {
  return form.value.permissions.includes(perm)
}

function toggle(perm: string) {
  const i = form.value.permissions.indexOf(perm)
  if (i >= 0) form.value.permissions.splice(i, 1)
  else form.value.permissions.push(perm)
}

function toggleModule(m: string) {
  const perms = actionsOf(m).map((a) => `${m}:${a}`)
  const allOn = perms.every((p) => form.value.permissions.includes(p))
  if (allOn) {
    form.value.permissions = form.value.permissions.filter((p) => !perms.includes(p))
  } else {
    for (const p of perms) if (!form.value.permissions.includes(p)) form.value.permissions.push(p)
  }
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [r, p] = await Promise.all([
      api.get('/api/admin/roles'),
      api.get('/api/admin/permissions'),
    ])
    roles.value = r.items
    allPermissions.value = p.items
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)

function openCreate() {
  editing.value = { builtin: false, isNew: true }
  form.value = { key: '', name: '', permissions: [] }
  error.value = ''
}

function openEdit(role: any) {
  editing.value = role
  form.value = { key: role.key, name: role.name, permissions: [...(role.permissions || [])] }
  error.value = ''
}

async function submit() {
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    if (editing.value?.isNew) {
      await api.post('/api/admin/roles', {
        key: form.value.key.trim(),
        name: form.value.name.trim(),
        permissions: form.value.permissions,
      })
      notice.value = '角色已创建'
    } else {
      await api.put(`/api/admin/roles/${editing.value.id}`, {
        name: form.value.name.trim(),
        permissions: form.value.permissions,
      })
      notice.value = '角色已更新'
    }
    editing.value = null
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function remove(role: any) {
  if (!confirm(`删除角色「${role.name}」？`)) return
  error.value = ''
  try {
    await api.del(`/api/admin/roles/${role.id}`)
    notice.value = '角色已删除'
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}
</script>

<template>
  <div class="row" style="justify-content: space-between; align-items: flex-start">
    <div>
      <h1>角色与权限</h1>
      <p class="muted small" style="margin-top: 4px">
        内置角色权限固定，不可修改；需要差异请新建自定义角色。权限判定取并集，没有 deny。
      </p>
    </div>
    <button v-if="can('role:write')" class="primary" @click="openCreate">+ 新建角色</button>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>
  <div v-if="loading" class="empty">加载中…</div>

  <div v-else class="grid" style="margin-top: 16px">
    <div v-for="role in roles" :key="role.id || role.key" class="panel stat">
      <div class="row" style="justify-content: space-between">
        <strong>{{ role.name }}</strong>
        <span class="tag" :class="role.builtin === false ? 'warn' : 'plain'">
          {{ role.builtin === false ? '自定义' : '内置' }}
        </span>
      </div>
      <div class="small muted mono">{{ role.key }}</div>
      <div class="small" style="margin-top: 8px">
        权限 {{ role.permissions?.length || 0 }} 项 · 成员 {{ role.memberCount }} 人
      </div>
      <div v-if="role.builtin === false && can('role:write')" class="row" style="gap: 6px; margin-top: 10px">
        <button class="tiny" @click="openEdit(role)">编辑权限</button>
        <button class="tiny danger" @click="remove(role)">删除</button>
      </div>
    </div>
  </div>

  <div v-if="editing" class="modal-mask" @click.self="editing = null">
    <div class="modal" style="max-width: 720px">
      <h2>{{ editing.isNew ? '新建角色' : '编辑角色' }}</h2>

      <label>标识（建后不可改）</label>
      <input v-model="form.key" :disabled="!editing.isNew" placeholder="auditor" />

      <label>名称</label>
      <input v-model="form.name" placeholder="审计员" />

      <label>权限（勾选即授予；同一模块可整列切换）</label>
      <table style="margin-top: 8px">
        <thead>
          <tr>
            <th style="width: 150px">模块</th>
            <th v-for="a in ACTION_ORDER" :key="a">{{ ACTION_LABEL[a] }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="m in modules" :key="m">
            <td>
              <span style="cursor: pointer" title="整行切换" @click="toggleModule(m)">
                {{ moduleLabel(m) }}
              </span>
            </td>
            <td v-for="a in ACTION_ORDER" :key="a">
              <input
                v-if="actionsOf(m).includes(a)"
                type="checkbox"
                style="width: auto"
                :checked="has(`${m}:${a}`)"
                @change="toggle(`${m}:${a}`)"
              />
              <span v-else class="muted small">—</span>
            </td>
          </tr>
        </tbody>
      </table>

      <div class="modal-actions">
        <button @click="editing = null">取消</button>
        <button class="primary" :disabled="busy" @click="submit">保存</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
.stat { padding: 16px; }
</style>
