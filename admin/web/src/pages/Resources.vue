<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { humanSize, when } from '../lib/format'
import { can } from '../store'
import TierMatrix from '../components/TierMatrix.vue'
import ResourceDrawer from '../components/ResourceDrawer.vue'
import ProviderForm from '../components/ProviderForm.vue'

const { t } = useI18n()
const route = useRoute()
const KINDS = [
  { key: 'provider', labelKey: 'kind.provider', read: 'provider:read' },
  { key: 'model', labelKey: 'kind.model', read: 'model:read' },
  { key: 'skill', labelKey: 'kind.skill', read: 'skill:read' },
  { key: 'plugin', labelKey: 'kind.plugin', read: 'plugin:read' },
  { key: 'mcp', labelKey: 'kind.mcp', read: 'mcp:read' },
  { key: 'tier_profile', labelKey: 'kind.tier_profile', read: 'model:read' },
]

const kind = ref('provider')
const items = ref<any[]>([])
const loading = ref(true)
const error = ref('')
const notice = ref('')

const roles = ref<any[]>([])
const members = ref<any[]>([])
// 档位映射矩阵需要模型与服务商列表
const models = ref<any[]>([])
const providers = ref<any[]>([])

const editing = ref<any>(null)
const creating = ref(false)
const form = ref({
  slug: '',
  name: '',
  description: '',
  payload: '{}',
  secret: '',
  enabled: false,
  activation: 'optional',
  visibility: 'all' as 'all' | 'specified',
})

const grantFor = ref<any>(null)
// 授权：支持角色与个人混排、可搜索勾选（设计图 06）
const grantAll = ref(true)
const grantTab = ref<'role' | 'user'>('role')
const grantSearch = ref('')
const grantRoleIds = ref<string[]>([])
const grantUserIds = ref<string[]>([])

const grantRoleOptions = computed(() => {
  const kw = grantSearch.value.trim().toLowerCase()
  return roles.value.filter(
    (r) => !kw || `${r.key} ${r.name || ''}`.toLowerCase().includes(kw)
  )
})
const grantUserOptions = computed(() => {
  const kw = grantSearch.value.trim().toLowerCase()
  return members.value.filter(
    (u) => !kw || `${u.username} ${u.displayName || ''}`.toLowerCase().includes(kw)
  )
})

// 列表：搜索 + 状态筛选 + 分页（设计图 03）
/** 顶栏搜索把关键词带过来（?q=），落地成这一页的筛选条件。 */
const keyword = ref(typeof route.query.q === 'string' ? route.query.q : '')
const statusFilter = ref<'all' | 'enabled' | 'disabled'>('all')
// 模型页按服务商过滤：模型一多，平铺一列根本找不到
const providerFilter = ref('')
const page = ref(1)
const PAGE_SIZE = 20

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  return items.value.filter((it) => {
    if (statusFilter.value === 'enabled' && !it.enabled) return false
    if (statusFilter.value === 'disabled' && it.enabled) return false
    if (providerFilter.value && it.parentId !== providerFilter.value) return false
    if (!kw) return true
    return `${it.slug} ${it.name}`.toLowerCase().includes(kw)
  })
})
const pageCount = computed(() => Math.max(1, Math.ceil(filtered.value.length / PAGE_SIZE)))
const paged = computed(() => {
  const start = (page.value - 1) * PAGE_SIZE
  return filtered.value.slice(start, start + PAGE_SIZE)
})
watch([keyword, statusFilter, kind], () => {
  page.value = 1
})

// 测试连接（设计图 07）
const testBusy = ref(false)
const testResult = ref<{ ok: boolean; message: string } | null>(null)

const revealed = ref<{ name: string; secret: string } | null>(null)
// 详情抽屉（设计图 05）
const detailFor = ref<any>(null)
// 行内「更多」菜单（把授权/版本/看密钥/停用/删除收起来，避免一排六个按钮挤成竖排）
const menuFor = ref<string | null>(null)

/** 模型所属服务商的显示名（列表里比父 id 前 8 位好读）。 */
function providerName(row: any) {
  const p = items.value.find((r: any) => r.id === row.parentId)
  return p?.name || ''
}

function toggleMenu(id: string) {
  menuFor.value = menuFor.value === id ? null : id
}

async function onProviderSaved() {
  creating.value = false
  editing.value = null
  notice.value = '模型服务已保存'
  await load()
}

function closeMenu() {
  menuFor.value = null
}

/** 点页面其它地方关掉菜单。 */
function onDocClick() {
  if (menuFor.value) menuFor.value = null
}

function onDrawerAction(row: any, action: 'edit' | 'grants' | 'versions' | 'toggle') {
  if (action === 'edit') openEdit(row)
  else if (action === 'grants') openGrants(row)
  else if (action === 'versions') openVersions(row)
  else toggleEnabled(row)
}

// 技能包版本
const versionsFor = ref<any>(null)
const versions = ref<any[]>([])
const uploadVersion = ref('')
const uploadFile = ref<File | null>(null)
const uploadBusy = ref(false)
const versionError = ref('')

const visibleKinds = computed(() => KINDS.filter((k) => can(k.read)))
const canWrite = computed(() => can(`${kind.value === 'tier_profile' ? 'model' : kind.value}:write`))

async function load() {
  loading.value = true
  error.value = ''
  try {
    const res = await api.get(`/api/admin/resources?kind=${kind.value}`)
    items.value = res.items
    // 模型页要按服务商筛；档位矩阵要选模型——两处都需要额外的列表
    if (kind.value === 'tier_profile' || kind.value === 'model') {
      const [m, p] = await Promise.all([
        api.get('/api/admin/resources?kind=model'),
        api.get('/api/admin/resources?kind=provider'),
      ])
      models.value = m.items
      providers.value = p.items
    }
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

function switchKind(next: string) {
  providerFilter.value = ''
  kind.value = next
  load()
}

function openCreate() {
  creating.value = true
  editing.value = null
  form.value = {
    slug: '', name: '', description: '', payload: '{}', secret: '',
    enabled: false, activation: 'optional', visibility: 'all',
  }
  testResult.value = null
}

function openEdit(row: any) {
  creating.value = false
  editing.value = row
  form.value = {
    slug: row.slug,
    name: row.name,
    description: row.description || '',
    payload: JSON.stringify(row.payload ?? {}, null, 2),
    secret: '',
    enabled: row.enabled,
    activation: row.activation || 'optional',
    visibility: 'all',
  }
  testResult.value = null
}

async function submit() {
  error.value = ''
  let payload: any
  try {
    payload = JSON.parse(form.value.payload || '{}')
  } catch {
    error.value = t('resources.badJson')
    return
  }
  try {
    if (creating.value) {
      const created = await api.post('/api/admin/resources', {
        kind: kind.value,
        slug: form.value.slug.trim(),
        name: form.value.name.trim(),
        description: form.value.description.trim(),
        payload,
        secret: form.value.secret || undefined,
        enabled: form.value.enabled,
        activation: form.value.activation,
      })
      notice.value = '资源已创建'
      // 建完直接给可见范围，省掉「建了但员工看不到」这一步（设计图 07）。
      if (form.value.visibility === 'all' && created?.id) {
        await api.put(`/api/admin/resources/${created.id}/grants`, {
          items: [{ type: 'all', id: '' }],
        })
      }
      creating.value = false
      editing.value = null
      await load()
      if (form.value.visibility === 'specified' && created?.id) {
        const row = items.value.find((it: any) => it.id === created.id)
        if (row) openGrants(row)
      }
      return
    } else {
      const body: any = {
        name: form.value.name.trim(),
        description: form.value.description.trim(),
        payload,
        enabled: form.value.enabled,
        activation: form.value.activation,
      }
      if (form.value.secret) body.secret = form.value.secret
      await api.put(`/api/admin/resources/${editing.value.id}`, body)
      notice.value = '资源已保存'
    }
    creating.value = false
    editing.value = null
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function toggleEnabled(row: any) {
  try {
    await api.put(`/api/admin/resources/${row.id}`, { enabled: !row.enabled })
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function remove(row: any) {
  if (!confirm(t('resources.deleteConfirm2', { name: row.name }))) return
  try {
    await api.del(`/api/admin/resources/${row.id}`)
    notice.value = t('resources.deleted')
    await load()
  } catch (e) {
    error.value = errText(e)
  }
}

async function openGrants(row: any) {
  grantFor.value = row
  grantAll.value = true
  grantTab.value = 'role'
  grantSearch.value = ''
  grantRoleIds.value = []
  grantUserIds.value = []
  try {
    const res = await api.get(`/api/admin/resources/${row.id}/grants`)
    const list: any[] = res.items || []
    grantAll.value = list.some((g) => g.type === 'all') || list.length === 0
    grantRoleIds.value = list.filter((g) => g.type === 'role').map((g) => g.id)
    grantUserIds.value = list.filter((g) => g.type === 'user').map((g) => g.id)
  } catch {
    /* 没有授权记录时保持「全员可见」默认 */
  }
}

function toggleGrantId(list: 'role' | 'user', id: string) {
  const target = list === 'role' ? grantRoleIds : grantUserIds
  const i = target.value.indexOf(id)
  if (i >= 0) target.value.splice(i, 1)
  else target.value.push(id)
}

/** 把当前选择拼成 grants 载荷。全员优先；否则角色 + 个人混排。 */
function grantItems() {
  if (grantAll.value) return [{ type: 'all', id: '' }]
  return [
    ...grantRoleIds.value.map((id) => ({ type: 'role', id })),
    ...grantUserIds.value.map((id) => ({ type: 'user', id })),
  ]
}

async function saveGrants() {
  try {
    await api.put(`/api/admin/resources/${grantFor.value.id}/grants`, { items: grantItems() })
    notice.value = '授权已保存'
    grantFor.value = null
  } catch (e) {
    error.value = errText(e)
  }
}

/** 测试连接：拿表单里的 baseUrl + key 让后端探一次 /models。 */
async function testConnection() {
  testResult.value = null
  let payload: any
  try {
    payload = JSON.parse(form.value.payload || '{}')
  } catch {
    testResult.value = { ok: false, message: t('resources.badJson') }
    return
  }
  const baseUrl = String(payload.baseUrl || '').trim()
  if (!baseUrl) {
    testResult.value = { ok: false, message: t('resources.needBaseUrl2') }
    return
  }
  testBusy.value = true
  try {
    const r = await api.post('/api/admin/providers/test', {
      baseUrl,
      apiKey: form.value.secret || '',
    })
    testResult.value = {
      ok: !!r.ok,
      message: r.ok ? `连接成功，发现 ${r.modelCount} 个模型` : r.message || '连接失败',
    }
  } catch (e) {
    testResult.value = { ok: false, message: errText(e) }
  } finally {
    testBusy.value = false
  }
}

async function reveal(row: any) {
  try {
    const res = await api.post(`/api/admin/resources/${row.id}/reveal`)
    revealed.value = { name: row.name, secret: res.secret }
  } catch (e) {
    error.value = errText(e)
  }
}

async function openVersions(row: any) {
  versionsFor.value = row
  versions.value = []
  uploadVersion.value = ''
  uploadFile.value = null
  versionError.value = ''
  await loadVersions(row.id)
}

async function loadVersions(resourceId: string) {
  try {
    const res = await api.get(`/api/admin/resources/${resourceId}/versions`)
    versions.value = res.items
  } catch (e) {
    versionError.value = errText(e)
  }
}

function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  uploadFile.value = input.files?.[0] ?? null
}

async function doUpload() {
  versionError.value = ''
  const file = uploadFile.value
  const version = uploadVersion.value.trim()
  if (!file) {
    versionError.value = '请选择 zip 文件'
    return
  }
  if (!version) {
    versionError.value = '请填写版本号'
    return
  }
  uploadBusy.value = true
  try {
    const res = await fetch(
      `/api/admin/resources/${versionsFor.value.id}/versions?version=${encodeURIComponent(version)}`,
      { method: 'POST', credentials: 'include', body: file },
    )
    const text = await res.text()
    if (!res.ok) {
      let message = t('resources.uploadFailed', { status: res.status })
      try {
        message = JSON.parse(text).message || message
      } catch {
        /* 非 JSON 错误体 */
      }
      throw new Error(message)
    }
    uploadVersion.value = ''
    uploadFile.value = null
    notice.value = t('resources.publishHint')
    await loadVersions(versionsFor.value.id)
    await load()
  } catch (e) {
    versionError.value = errText(e)
  } finally {
    uploadBusy.value = false
  }
}

async function publishVersion(version: any) {
  versionError.value = ''
  try {
    await api.post(`/api/admin/resources/${versionsFor.value.id}/versions/${version.id}/publish`)
    notice.value = t('resources.publishedV', { v: version.version })
    await loadVersions(versionsFor.value.id)
    await load()
  } catch (e) {
    versionError.value = errText(e)
  }
}

async function removeVersion(version: any) {
  if (!confirm(t('resources.deleteVersion2', { v: version.version }))) return
  versionError.value = ''
  try {
    await api.del(`/api/admin/resources/${versionsFor.value.id}/versions/${version.id}`)
    await loadVersions(versionsFor.value.id)
  } catch (e) {
    versionError.value = errText(e)
  }
}

watch(
  () => route.query.q,
  (q) => {
    keyword.value = typeof q === 'string' ? q : ''
  }
)

onMounted(async () => {
  try {
    const [roleList, userList] = await Promise.all([
      api.get('/api/admin/roles'),
      api.get('/api/admin/users'),
    ])
    roles.value = roleList.items
    members.value = userList.items
  } catch {
    /* 没有角色/成员读取权限时下拉为空，不影响资源列表 */
  }
  await load()
})
</script>

<template>
  <div class="spread">
    <div>
      <p class="muted small" style="margin: 0">{{ t('resources.subtitle') }}</p>
    </div>
    <!-- 模型不单独建：它属于某个服务商，跟着服务商一起配 -->
    <span v-if="kind === 'model'" class="small muted">
      {{ t('resources.modelsWithProvider') }}
    </span>
    <button v-else-if="canWrite" class="primary" @click="openCreate">
      {{ t('resources.new') }}
    </button>
  </div>

  <div class="tabs" style="margin-top: 16px">
    <button
      v-for="k in visibleKinds"
      :key="k.key"
      :class="{ active: kind === k.key }"
      @click="switchKind(k.key)"
    >{{ t(k.labelKey) }}</button>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>

  <!-- 档位映射用矩阵编辑器，不走通用表格（设计图 08/09） -->
  <TierMatrix
    v-if="kind === 'tier_profile'"
    :resource="items[0] || null"
    :models="models"
    :providers="providers"
    :can-write="canWrite"
    @saved="load"
  />

  <div v-else class="panel" style="margin-top: 16px">
    <!-- 设计图 03：搜索 + 状态筛选 -->
    <div class="row" style="padding: 0 0 12px; gap: 8px">
      <input v-model="keyword" :placeholder="t('resources.search')" style="max-width: 260px" />
      <select
        v-if="kind === 'model'"
        v-model="providerFilter"
        style="max-width: 180px"
      >
        <option value="">{{ t('resources.allProviders') }}</option>
        <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <select v-model="statusFilter" style="max-width: 140px">
        <option value="all">{{ t('resources.allStatus') }}</option>
        <option value="enabled">{{ t('common.enabled') }}</option>
        <option value="disabled">{{ t('common.disabled') }}</option>
      </select>
      <span class="small muted">
        共 {{ filtered.length }} 条 · 已启用 {{ filtered.filter((i) => i.enabled).length }}
      </span>
    </div>

    <div v-if="loading" class="empty">{{ t('common.loading') }}</div>
    <div v-else-if="!items.length" class="empty">{{ t('resources.empty') }}</div>
    <div v-else-if="!filtered.length" class="empty">{{ t('resources.noMatch') }}</div>
    <table v-else>
      <thead>
        <tr>
          <th style="width: 190px">{{ t('resources.id') }}</th>
          <th>{{ t('resources.name') }}</th>
          <th style="width: 88px">{{ t('resources.status') }}</th>
          <th style="width: 88px">{{ t('resources.secret') }}</th>
          <th style="width: 140px">{{ t('resources.updated') }}</th>
          <th style="width: 176px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in paged" :key="row.id">
          <td class="mono">{{ row.slug }}</td>
          <td>
            {{ row.name }}
            <div v-if="row.kind === 'model' && providerName(row)" class="small muted">
              来自 {{ providerName(row) }}
            </div>
          </td>
          <td>
            <span class="tag" :class="row.enabled ? 'ok' : 'plain'">
              {{ row.enabled ? t('common.enabled') : t('resources.disableShort') }}
            </span>
          </td>
          <td>
            <span v-if="row.hasSecret" class="tag warn">已配置</span>
            <span v-else class="muted small">—</span>
          </td>
          <td class="small muted">{{ when(row.updatedAt) }}</td>
          <td class="actions">
            <div class="actions-row">
              <button class="tiny" @click="detailFor = row">{{ t('resources.detail') }}</button>
              <button v-if="canWrite" class="tiny" @click="openEdit(row)">{{ t('resources.edit') }}</button>
              <div class="more-wrap">
                <button class="tiny" @click.stop="toggleMenu(row.id)">{{ t('resources.more') }}</button>
                <div v-if="menuFor === row.id" class="more-menu" @click.stop>
                  <button v-if="canWrite" @click="closeMenu(); openGrants(row)">{{ t('resources.grants') }}</button>
                  <button v-if="row.kind === 'skill' && can('skill:read')" @click="closeMenu(); openVersions(row)">
                    版本历史
                  </button>
                  <button v-if="can('provider:reveal') && row.hasSecret" @click="closeMenu(); reveal(row)">
                    查看密钥
                  </button>
                  <button v-if="canWrite" @click="closeMenu(); toggleEnabled(row)">
                    {{ row.enabled ? t('resources.disable') : t('resources.enable') }}
                  </button>
                  <button v-if="canWrite" class="danger" @click="closeMenu(); remove(row)">{{ t('resources.deleteResource') }}</button>
                </div>
              </div>
            </div>
          </td>
        </tr>
      </tbody>
    </table>

    <!-- 分页（设计图 03） -->
    <div
      v-if="pageCount > 1"
      class="row"
      style="padding-top: 12px; gap: 8px; align-items: center"
    >
      <button class="tiny" :disabled="page <= 1" @click="page -= 1">{{ t('resources.prev') }}</button>
      <span class="small muted">{{ t('resources.page', { page, total: pageCount }) }}</span>
      <button class="tiny" :disabled="page >= pageCount" @click="page += 1">{{ t('resources.next') }}</button>
    </div>
  </div>

  <ResourceDrawer
    v-if="detailFor"
    :resource="detailFor"
    :can-write="canWrite"
    @close="detailFor = null"
    @action="onDrawerAction(detailFor, $event)"
    @refresh="load"
  />

  <ProviderForm
    v-if="(creating || editing) && kind === 'provider'"
    :resource="editing"
    @saved="onProviderSaved"
    @cancel="creating = false; editing = null"
  />

  <div
    v-if="(creating || editing) && kind !== 'provider'"
    class="modal-mask"
    @click.self="creating = false; editing = null"
  >
    <div class="modal">
      <h2>{{ creating ? t('resources.new') : t('resources.editTitle') }}</h2>

      <label>标识（slug，建后不可改）</label>
      <input v-model="form.slug" :disabled="!creating" placeholder="qwen / qwen3.5-plus" />

      <label>{{ t('resources.name') }}</label>
      <input v-model="form.name" placeholder="阿里云百炼" />

      <label>{{ t('resources.desc') }}</label>
      <input v-model="form.description" :placeholder="t('resources.optional')" />

      <label>payload（JSON）</label>
      <textarea v-model="form.payload" rows="6" class="mono"></textarea>
      <p class="small muted" style="margin: 4px 0 0">
        provider 用 <span class="mono">{"baseUrl":"https://…"}</span>；档位映射写
        <span class="mono">{"agentModeLlm":{…}}</span>。
      </p>

      <label>{{ t('resources.secret') }}{{ editing ? t('resources.secretKeep') : '' }}</label>
      <input v-model="form.secret" type="password" placeholder="sk-…" />
      <div v-if="kind === 'provider'" class="row" style="gap: 8px; margin-top: 8px">
        <button class="tiny" :disabled="testBusy" @click="testConnection">
          {{ testBusy ? '测试中…' : '测试连接' }}
        </button>
        <span
          v-if="testResult"
          class="small"
          :style="{ color: testResult.ok ? 'var(--ok)' : 'var(--danger)' }"
        >
          {{ testResult.message }}
        </span>
      </div>

      <label>{{ t('drawer.visibleTo') }}</label>
      <select v-model="form.visibility">
        <option value="all">{{ t('resources.grantEveryone') }}</option>
        <option value="specified">指定角色 / 成员（保存后弹出选择）</option>
      </select>
      <p class="small muted" style="margin: 4px 0 0">
        不设可见范围时员工端拿不到这个资源——建完不用再点一次「授权」。
      </p>

      <label class="row" style="gap: 8px; margin-top: 16px">
        <input type="checkbox" v-model="form.enabled" style="width: auto" />
        <span class="small">{{ t('resources.enableNow') }}</span>
      </label>

      <label v-if="kind === 'skill' || kind === 'plugin'">{{ t('resources.activation') }}</label>
      <select v-if="kind === 'skill' || kind === 'plugin'" v-model="form.activation">
        <option value="optional">{{ t('resources.activationOptional') }}</option>
        <option value="default_on">{{ t('resources.activationDefaultOn') }}</option>
      </select>
      <p v-if="kind === 'skill' || kind === 'plugin'" class="small muted" style="margin: 4px 0 0">
        插件即使设为 default_on 也只落盘、不自动激活（插件带工具与 MCP，需显式授权）。
      </p>

      <div class="modal-actions">
        <button @click="creating = false; editing = null">{{ t('common.cancel') }}</button>
        <button class="primary" @click="submit">{{ t('common.save') }}</button>
      </div>
    </div>
  </div>

  <div v-if="grantFor" class="modal-mask" @click.self="grantFor = null">
    <div class="modal">
      <h2>{{ t('resources.grantFor', { name: grantFor.name }) }}</h2>
      <p class="muted small">{{ t('resources.grantHint') }}</p>

      <label class="row" style="gap: 8px; margin-top: 14px">
        <input type="radio" :value="true" v-model="grantAll" style="width: auto" />
        <span>{{ t('resources.grantEveryone') }}</span>
      </label>
      <label class="row" style="gap: 8px">
        <input type="radio" :value="false" v-model="grantAll" style="width: auto" />
        <span>{{ t('resources.grantSpecific') }}</span>
      </label>

      <div v-if="!grantAll" style="margin-top: 12px">
        <!-- 已选 chips：角色与个人混排 -->
        <div v-if="grantRoleIds.length || grantUserIds.length" class="row" style="gap: 6px; flex-wrap: wrap; margin-bottom: 10px">
          <span v-for="id in grantRoleIds" :key="'r' + id" class="tag ok">
            角色 · {{ roles.find((r) => r.id === id)?.name || id }}
            <span style="cursor: pointer; margin-left: 4px" @click="toggleGrantId('role', id)">×</span>
          </span>
          <span v-for="id in grantUserIds" :key="'u' + id" class="tag" style="background: var(--accent-soft); color: var(--accent)">
            个人 · {{ members.find((m) => m.id === id)?.displayName || id }}
            <span style="cursor: pointer; margin-left: 4px" @click="toggleGrantId('user', id)">×</span>
          </span>
        </div>

        <div class="row" style="gap: 6px; margin-bottom: 8px">
          <button class="tiny" :class="{ primary: grantTab === 'role' }" @click="grantTab = 'role'">{{ t('resources.grantByRole') }}</button>
          <button class="tiny" :class="{ primary: grantTab === 'user' }" @click="grantTab = 'user'">{{ t('resources.grantByMember') }}</button>
        </div>

        <input v-model="grantSearch" :placeholder="grantTab === 'role' ? t('resources.searchRole') : t('resources.searchMember')" />

        <div style="max-height: 220px; overflow: auto; margin-top: 8px; border: 1px solid var(--line); border-radius: var(--radius-sm)">
          <template v-if="grantTab === 'role'">
            <label
              v-for="r in grantRoleOptions"
              :key="r.id"
              class="row"
              style="gap: 8px; padding: 7px 10px; border-bottom: 1px solid var(--line)"
            >
              <input
                type="checkbox"
                style="width: auto"
                :checked="grantRoleIds.includes(r.id)"
                @change="toggleGrantId('role', r.id)"
              />
              <span class="small">{{ r.name }} <span class="muted">（{{ r.key }}）</span></span>
            </label>
            <div v-if="!grantRoleOptions.length" class="empty small">{{ t('resources.noRole') }}</div>
          </template>
          <template v-else>
            <label
              v-for="m in grantUserOptions"
              :key="m.id"
              class="row"
              style="gap: 8px; padding: 7px 10px; border-bottom: 1px solid var(--line)"
            >
              <input
                type="checkbox"
                style="width: auto"
                :checked="grantUserIds.includes(m.id)"
                @change="toggleGrantId('user', m.id)"
              />
              <span class="small">
                {{ m.displayName || m.username }}
                <span class="muted mono">{{ m.username }}</span>
              </span>
            </label>
            <div v-if="!grantUserOptions.length" class="empty small">{{ t('resources.noMember') }}</div>
          </template>
        </div>
      </div>

      <div class="modal-actions">
        <button @click="grantFor = null">{{ t('common.cancel') }}</button>
        <button class="primary" @click="saveGrants">{{ t('resources.saveGrants') }}</button>
      </div>
    </div>
  </div>

  <div v-if="revealed" class="modal-mask" @click.self="revealed = null">
    <div class="modal">
      <h2>{{ t('resources.revealed', { name: revealed.name }) }}</h2>
      <p class="err small" style="margin-top: 12px">{{ t('resources.audited') }}</p>
      <pre class="mono" style="margin-top: 12px; padding: 10px; border: 1px solid var(--line); border-radius: 8px; overflow: auto">{{ revealed.secret }}</pre>
      <div class="modal-actions">
        <button @click="revealed = null">{{ t('common.close') }}</button>
      </div>
    </div>
  </div>

  <div v-if="versionsFor" class="modal-mask" @click.self="versionsFor = null">
    <div class="modal" style="width: 620px">
      <h2>{{ t('resources.versions', { name: versionsFor.name }) }}</h2>
      <p class="muted small">
        {{ t('resources.uploadHint') }} <span class="mono">SKILL.md</span>，
        上限 20 MB，单文件 5 MB。
      </p>

      <div v-if="versionError" class="err" style="margin-top: 12px">{{ versionError }}</div>

      <div v-if="can('skill:write')" class="upload-box">
        <div style="flex: 1">
          <label>{{ t('resources.zipFile') }}</label>
          <input type="file" accept=".zip" @change="pickFile" />
        </div>
        <div style="width: 140px">
          <label>{{ t('resources.versionNo') }}</label>
          <input v-model="uploadVersion" placeholder="1.0.0" class="mono" />
        </div>
        <button class="primary" style="margin-top: 18px" :disabled="uploadBusy" @click="doUpload">
          {{ uploadBusy ? t('resources.uploading') : t('resources.upload') }}
        </button>
      </div>

      <div v-if="!versions.length" class="empty">{{ t('resources.noVersions') }}</div>
      <table v-else style="margin-top: 12px">
        <thead>
          <tr>
            <th>{{ t('resources.version') }}</th>
            <th>{{ t('resources.contentHash') }}</th>
            <th class="num">{{ t('resources.size') }}</th>
            <th style="width: 150px"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="v in versions" :key="v.id">
            <td class="mono">
              {{ v.version }}
              <span v-if="v.isCurrent" class="tag ok" style="margin-left: 6px">{{ t('resources.published') }}</span>
            </td>
            <td class="mono small">{{ v.hash.slice(0, 12) }}…</td>
            <td class="num small">{{ humanSize(v.sizeBytes) }}</td>
            <td class="row" style="gap: 6px">
              <button
                v-if="can('skill:publish') && !v.isCurrent"
                class="tiny"
                @click="publishVersion(v)"
              >{{ t('resources.publish') }}</button>
              <button
                v-if="can('skill:write') && !v.isCurrent"
                class="tiny danger"
                @click="removeVersion(v)"
              >{{ t('common.delete') }}</button>
            </td>
          </tr>
        </tbody>
      </table>

      <div class="modal-actions">
        <button @click="versionsFor = null">{{ t('common.close') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.upload-box {
  display: flex;
  gap: 12px;
  align-items: flex-end;
  margin-top: 14px;
  padding: 14px;
  background: #fafbfc;
  border: 1px solid var(--line);
  border-radius: 8px;
}

.more-wrap { position: relative; }
.more-menu {
  position: absolute;
  right: 0;
  top: calc(100% + 4px);
  min-width: 132px;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(24, 24, 27, 0.12);
  padding: 4px;
  z-index: 30;
  display: flex;
  flex-direction: column;
}
.more-menu button {
  border: 0;
  background: transparent;
  text-align: left;
  padding: 7px 10px;
  border-radius: 6px;
  font-size: 13px;
}
.more-menu button:hover { background: var(--panel-alt); }
.more-menu button.danger { color: var(--danger); }
</style>
