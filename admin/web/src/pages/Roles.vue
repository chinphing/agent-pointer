<script setup lang="ts">
/**
 * 角色与权限（设计图 04）。
 *
 * 内置角色是代码常量，权限随版本走——只读展示，改了会被下次升级覆盖。
 * 自定义角色存在 roles / role_permissions，可增删改；权限判定只读后者，
 * 所以建完立刻生效。
 */
import { computed, onMounted, ref } from 'vue'
import { useNotice } from '../lib/notice'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { can } from '../store'
import { roleLabel } from '../lib/roles'

const { t } = useI18n()

/** 模块与动作的名字走语言包；新增模块只要加键，不用改这里。 */
const MODULE_ORDER = ['user', 'role', 'provider', 'model', 'skill', 'plugin', 'mcp', 'usage', 'audit', 'system']
const ACTION_ORDER = ['read', 'write', 'publish', 'delete', 'reveal']

function moduleLabel(m: string) {
  const key = `permModule.${m}`
  return t(key) === key ? m : t(key)
}
function actionLabel(a: string) {
  const key = `permAction.${a}`
  return t(key) === key ? a : t(key)
}
/** 角色名：内置角色翻译，自定义角色用管理员起的名字。 */
function nameOf(role: any) {
  return roleLabel(role.key, role.name, t)
}

const roles = ref<any[]>([])
const allPermissions = ref<string[]>([])
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()
const busy = ref(false)

// 编辑弹窗
const editing = ref<any | null>(null)
const form = ref({ key: '', name: '', permissions: [] as string[] })

const modules = computed(() => {
  const set = new Set<string>()
  for (const p of allPermissions.value) set.add(p.split(':')[0])
  return [...set].sort((a, b) => {
    const ai = MODULE_ORDER.indexOf(a)
    const bi = MODULE_ORDER.indexOf(b)
    return (ai < 0 ? 99 : ai) - (bi < 0 ? 99 : bi)
  })
})

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

/** 只读查看：内置角色也能看自己有哪些权限。 */
function openView(role: any) {
  editing.value = { ...role, readonly: true, isNew: false }
  form.value = { key: role.key, name: role.name, permissions: [...(role.permissions || [])] }
  error.value = ''
}

/** 内置角色是代码常量、改了会被升级覆盖，所以提供「复制成自定义角色」再改。 */
function cloneAsCustom(role: any) {
  editing.value = { builtin: false, isNew: true }
  form.value = {
    key: `${role.key}_copy`,
    name: `${nameOf(role)}（副本）`,
    permissions: [...(role.permissions || [])],
  }
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
  flashNotice('')
  try {
    if (editing.value?.isNew) {
      await api.post('/api/admin/roles', {
        key: form.value.key.trim(),
        name: form.value.name.trim(),
        permissions: form.value.permissions,
      })
      flashNotice(t('roles_page.created'))
    } else {
      await api.put(`/api/admin/roles/${editing.value.id}`, {
        name: form.value.name.trim(),
        permissions: form.value.permissions,
      })
      flashNotice(t('roles_page.updated'))
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
  if (!confirm(t('roles_page.deleteConfirm', { name: nameOf(role) }))) return
  error.value = ''
  try {
    await api.del(`/api/admin/roles/${role.id}`)
    flashNotice(t('roles_page.deleted'))
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}
</script>

<template>
  <div class="row" style="justify-content: space-between; align-items: flex-start">
    <div>
      <p class="muted small" style="margin: 0">{{ t('roles_page.subtitle') }}</p>
    </div>
    <button v-if="can('role:write')" class="primary" @click="openCreate">
      {{ t('roles_page.create') }}
    </button>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>
  <div v-if="loading" class="empty">{{ t('common.loading') }}</div>

  <div v-else class="grid" style="margin-top: 16px">
    <div v-for="role in roles" :key="role.id || role.key" class="panel stat">
      <div class="row" style="justify-content: space-between">
        <strong>{{ nameOf(role) }}</strong>
        <span class="tag" :class="role.builtin === false ? 'warn' : 'plain'">
          {{ role.builtin === false ? t('roles.custom') : t('roles.builtin') }}
        </span>
      </div>
      <div class="small muted mono">{{ role.key }}</div>
      <div class="small" style="margin-top: 8px">
        {{ t('roles_page.permissionCount', { n: role.permissions?.length || 0 }) }}
        · {{ t('roles_page.memberCount', { n: role.memberCount }) }}
      </div>
      <div class="row" style="gap: 6px; margin-top: 10px">
        <button class="tiny" @click="openView(role)">{{ t('roles_page.viewPerm') }}</button>
        <template v-if="role.builtin === false && can('role:write')">
          <button class="tiny" @click="openEdit(role)">{{ t('roles_page.editPerm') }}</button>
          <button class="tiny danger" @click="remove(role)">{{ t('common.delete') }}</button>
        </template>
        <button v-else-if="can('role:write')" class="tiny" @click="cloneAsCustom(role)">
          {{ t('roles_page.cloneAsCustom') }}
        </button>
      </div>
    </div>
  </div>

  <div v-if="editing" class="modal-mask" @click.self="editing = null">
    <div class="modal" style="max-width: 720px">
      <h2>
        {{
          editing.readonly
            ? t('roles_page.viewTitle', { name: nameOf(editing) })
            : editing.isNew
              ? t('roles_page.createTitle')
              : t('roles_page.editTitle')
        }}
      </h2>
      <p v-if="editing.readonly" class="muted small">{{ t('roles_page.builtinReadonlyHint') }}</p>

      <label>{{ t('roles_page.keyLabel') }}</label>
      <input
        v-model="form.key"
        :disabled="!editing.isNew || editing.readonly"
        :placeholder="t('roles_page.keyPlaceholder')"
      />

      <label>{{ t('roles_page.nameLabel') }}</label>
      <input
        v-model="form.name"
        :disabled="editing.readonly"
        :placeholder="t('roles_page.namePlaceholder')"
      />

      <label>{{ t('roles_page.permissionHint') }}</label>
      <table style="margin-top: 8px">
        <thead>
          <tr>
            <th style="width: 150px">{{ t('roles_page.moduleCol') }}</th>
            <th v-for="a in ACTION_ORDER" :key="a">{{ actionLabel(a) }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="m in modules" :key="m">
            <td>
              <span style="cursor: pointer" :title="t('roles_page.toggleRow')" @click="toggleModule(m)">
                {{ moduleLabel(m) }}
              </span>
            </td>
            <td v-for="a in ACTION_ORDER" :key="a">
              <input
                v-if="actionsOf(m).includes(a)"
                type="checkbox"
                style="width: auto"
                :checked="has(`${m}:${a}`)"
                :disabled="editing.readonly"
                @change="toggle(`${m}:${a}`)"
              />
              <span v-else class="muted small">—</span>
            </td>
          </tr>
        </tbody>
      </table>

      <div class="modal-actions">
        <button @click="editing = null">
          {{ editing.readonly ? t('common.close') : t('common.cancel') }}
        </button>
        <button
          v-if="editing.readonly && editing.builtin !== false && can('role:write')"
          class="primary"
          @click="cloneAsCustom(editing)"
        >
          {{ t('roles_page.cloneAsCustom') }}
        </button>
        <button
          v-else-if="!editing.readonly"
          class="primary"
          :disabled="busy"
          @click="submit"
        >
          {{ t('common.save') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
.stat { padding: 16px; }
</style>
