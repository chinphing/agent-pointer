<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { api, errText } from '../api'
import { humanSize, when } from '../lib/format'
import { can } from '../store'
import TierMatrix from '../components/TierMatrix.vue'

const KINDS = [
  { key: 'provider', label: '模型服务', read: 'provider:read' },
  { key: 'model', label: '模型', read: 'model:read' },
  { key: 'skill', label: '技能', read: 'skill:read' },
  { key: 'plugin', label: '插件', read: 'plugin:read' },
  { key: 'mcp', label: 'MCP', read: 'mcp:read' },
  { key: 'tier_profile', label: '档位映射', read: 'model:read' },
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
const keyword = ref('')
const statusFilter = ref<'all' | 'enabled' | 'disabled'>('all')
const page = ref(1)
const PAGE_SIZE = 20

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  return items.value.filter((it) => {
    if (statusFilter.value === 'enabled' && !it.enabled) return false
    if (statusFilter.value === 'disabled' && it.enabled) return false
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
    if (kind.value === 'tier_profile') {
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
    error.value = 'payload 不是合法 JSON'
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
  if (!confirm(`删除资源「${row.name}」？授权关系会一并清除。`)) return
  try {
    await api.del(`/api/admin/resources/${row.id}`)
    notice.value = '已删除'
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
    testResult.value = { ok: false, message: 'payload 不是合法 JSON' }
    return
  }
  const baseUrl = String(payload.baseUrl || '').trim()
  if (!baseUrl) {
    testResult.value = { ok: false, message: '先填 payload 里的 baseUrl' }
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
      let message = `上传失败（HTTP ${res.status}）`
      try {
        message = JSON.parse(text).message || message
      } catch {
        /* 非 JSON 错误体 */
      }
      throw new Error(message)
    }
    uploadVersion.value = ''
    uploadFile.value = null
    notice.value = '版本已上传，记得点「发布」才会下发给员工'
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
    notice.value = `已发布 ${version.version}`
    await loadVersions(versionsFor.value.id)
    await load()
  } catch (e) {
    versionError.value = errText(e)
  }
}

async function removeVersion(version: any) {
  if (!confirm(`删除版本 ${version.version}？`)) return
  versionError.value = ''
  try {
    await api.del(`/api/admin/resources/${versionsFor.value.id}/versions/${version.id}`)
    await loadVersions(versionsFor.value.id)
  } catch (e) {
    versionError.value = errText(e)
  }
}

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
      <h1>资源</h1>
      <p class="muted small" style="margin: 4px 0 0">
        服务商 / 模型 / 技能 / 插件 / MCP / 档位映射共用一套存储与授权。
      </p>
    </div>
    <button v-if="canWrite" class="primary" @click="openCreate">新建{{ KINDS.find((k) => k.key === kind)?.label }}</button>
  </div>

  <div class="tabs" style="margin-top: 16px">
    <button
      v-for="k in visibleKinds"
      :key="k.key"
      :class="{ active: kind === k.key }"
      @click="switchKind(k.key)"
    >{{ k.label }}</button>
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
      <input v-model="keyword" placeholder="搜索名称 / 标识" style="max-width: 260px" />
      <select v-model="statusFilter" style="max-width: 140px">
        <option value="all">状态：全部</option>
        <option value="enabled">已启用</option>
        <option value="disabled">已停用</option>
      </select>
      <span class="small muted">
        共 {{ filtered.length }} 条 · 已启用 {{ filtered.filter((i) => i.enabled).length }}
      </span>
    </div>

    <div v-if="loading" class="empty">加载中…</div>
    <div v-else-if="!items.length" class="empty">这一类还没有资源</div>
    <div v-else-if="!filtered.length" class="empty">没有匹配的资源</div>
    <table v-else>
      <thead>
        <tr>
          <th>标识</th>
          <th>名称</th>
          <th style="width: 90px">状态</th>
          <th style="width: 90px">密钥</th>
          <th style="width: 170px">更新时间</th>
          <th style="width: 210px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in paged" :key="row.id">
          <td class="mono">{{ row.slug }}</td>
          <td>
            {{ row.name }}
            <div v-if="row.parentId" class="small muted mono">父：{{ row.parentId.slice(0, 8) }}</div>
          </td>
          <td>
            <span class="tag" :class="row.enabled ? 'ok' : 'plain'">
              {{ row.enabled ? '已启用' : '停用' }}
            </span>
          </td>
          <td>
            <span v-if="row.hasSecret" class="tag warn">已配置</span>
            <span v-else class="muted small">—</span>
          </td>
          <td class="small muted">{{ when(row.updatedAt) }}</td>
          <td class="row" style="gap: 6px">
            <button v-if="canWrite" class="tiny" @click="openEdit(row)">编辑</button>
            <button v-if="canWrite" class="tiny" @click="openGrants(row)">授权</button>
            <button v-if="row.kind === 'skill' && can('skill:read')" class="tiny" @click="openVersions(row)">版本</button>
            <button v-if="can('provider:reveal') && row.hasSecret" class="tiny" @click="reveal(row)">看密钥</button>
            <button v-if="canWrite" class="tiny" @click="toggleEnabled(row)">
              {{ row.enabled ? '停用' : '启用' }}
            </button>
            <button v-if="canWrite" class="tiny danger" @click="remove(row)">删除</button>
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
      <button class="tiny" :disabled="page <= 1" @click="page -= 1">上一页</button>
      <span class="small muted">第 {{ page }} / {{ pageCount }} 页</span>
      <button class="tiny" :disabled="page >= pageCount" @click="page += 1">下一页</button>
    </div>
  </div>

  <div v-if="creating || editing" class="modal-mask" @click.self="creating = false; editing = null">
    <div class="modal">
      <h2>{{ creating ? '新建资源' : '编辑资源' }}</h2>

      <label>标识（slug，建后不可改）</label>
      <input v-model="form.slug" :disabled="!creating" placeholder="qwen / qwen3.5-plus" />

      <label>名称</label>
      <input v-model="form.name" placeholder="阿里云百炼" />

      <label>描述</label>
      <input v-model="form.description" placeholder="可选" />

      <label>payload（JSON）</label>
      <textarea v-model="form.payload" rows="6" class="mono"></textarea>
      <p class="small muted" style="margin: 4px 0 0">
        provider 用 <span class="mono">{"baseUrl":"https://…"}</span>；档位映射写
        <span class="mono">{"agentModeLlm":{…}}</span>。
      </p>

      <label>密钥{{ editing ? '（留空表示不修改）' : '' }}</label>
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

      <label>可见范围</label>
      <select v-model="form.visibility">
        <option value="all">全员可见</option>
        <option value="specified">指定角色 / 成员（保存后弹出选择）</option>
      </select>
      <p class="small muted" style="margin: 4px 0 0">
        不设可见范围时员工端拿不到这个资源——建完不用再点一次「授权」。
      </p>

      <label class="row" style="gap: 8px; margin-top: 16px">
        <input type="checkbox" v-model="form.enabled" style="width: auto" />
        <span class="small">立即启用（新建时默认停用）</span>
      </label>

      <label v-if="kind === 'skill' || kind === 'plugin'">启用策略</label>
      <select v-if="kind === 'skill' || kind === 'plugin'" v-model="form.activation">
        <option value="optional">optional — 员工自己决定（默认）</option>
        <option value="default_on">default_on — 下发后默认启用，员工可关</option>
      </select>
      <p v-if="kind === 'skill' || kind === 'plugin'" class="small muted" style="margin: 4px 0 0">
        插件即使设为 default_on 也只落盘、不自动激活（插件带工具与 MCP，需显式授权）。
      </p>

      <div class="modal-actions">
        <button @click="creating = false; editing = null">取消</button>
        <button class="primary" @click="submit">保存</button>
      </div>
    </div>
  </div>

  <div v-if="grantFor" class="modal-mask" @click.self="grantFor = null">
    <div class="modal">
      <h2>授权：{{ grantFor.name }}</h2>
      <p class="muted small">只有并集，没有排除。选「全员」时不要再选角色/成员。</p>

      <label class="row" style="gap: 8px; margin-top: 14px">
        <input type="radio" :value="true" v-model="grantAll" style="width: auto" />
        <span>全员可见</span>
      </label>
      <label class="row" style="gap: 8px">
        <input type="radio" :value="false" v-model="grantAll" style="width: auto" />
        <span>按角色 / 成员指定</span>
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
          <button class="tiny" :class="{ primary: grantTab === 'role' }" @click="grantTab = 'role'">按角色</button>
          <button class="tiny" :class="{ primary: grantTab === 'user' }" @click="grantTab = 'user'">按成员</button>
        </div>

        <input v-model="grantSearch" :placeholder="grantTab === 'role' ? '搜索角色' : '搜索姓名或账号'" />

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
            <div v-if="!grantRoleOptions.length" class="empty small">没有匹配的角色</div>
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
            <div v-if="!grantUserOptions.length" class="empty small">没有匹配的成员</div>
          </template>
        </div>
      </div>

      <div class="modal-actions">
        <button @click="grantFor = null">取消</button>
        <button class="primary" @click="saveGrants">保存授权</button>
      </div>
    </div>
  </div>

  <div v-if="revealed" class="modal-mask" @click.self="revealed = null">
    <div class="modal">
      <h2>明文密钥：{{ revealed.name }}</h2>
      <p class="err small" style="margin-top: 12px">这次查看已经记入审计。</p>
      <pre class="mono" style="margin-top: 12px; padding: 10px; border: 1px solid var(--line); border-radius: 8px; overflow: auto">{{ revealed.secret }}</pre>
      <div class="modal-actions">
        <button @click="revealed = null">关闭</button>
      </div>
    </div>
  </div>

  <div v-if="versionsFor" class="modal-mask" @click.self="versionsFor = null">
    <div class="modal" style="width: 620px">
      <h2>版本：{{ versionsFor.name }}</h2>
      <p class="muted small">
        上传 zip 后需要点「发布」，员工端才会拿到。包必须包含 <span class="mono">SKILL.md</span>，
        上限 20 MB，单文件 5 MB。
      </p>

      <div v-if="versionError" class="err" style="margin-top: 12px">{{ versionError }}</div>

      <div v-if="can('skill:write')" class="upload-box">
        <div style="flex: 1">
          <label>zip 文件</label>
          <input type="file" accept=".zip" @change="pickFile" />
        </div>
        <div style="width: 140px">
          <label>版本号</label>
          <input v-model="uploadVersion" placeholder="1.0.0" class="mono" />
        </div>
        <button class="primary" style="margin-top: 18px" :disabled="uploadBusy" @click="doUpload">
          {{ uploadBusy ? '上传中…' : '上传' }}
        </button>
      </div>

      <div v-if="!versions.length" class="empty">还没有版本</div>
      <table v-else style="margin-top: 12px">
        <thead>
          <tr>
            <th>版本</th>
            <th>内容哈希</th>
            <th class="num">大小</th>
            <th style="width: 150px"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="v in versions" :key="v.id">
            <td class="mono">
              {{ v.version }}
              <span v-if="v.isCurrent" class="tag ok" style="margin-left: 6px">已发布</span>
            </td>
            <td class="mono small">{{ v.hash.slice(0, 12) }}…</td>
            <td class="num small">{{ humanSize(v.sizeBytes) }}</td>
            <td class="row" style="gap: 6px">
              <button
                v-if="can('skill:publish') && !v.isCurrent"
                class="tiny"
                @click="publishVersion(v)"
              >发布</button>
              <button
                v-if="can('skill:write') && !v.isCurrent"
                class="tiny danger"
                @click="removeVersion(v)"
              >删除</button>
            </td>
          </tr>
        </tbody>
      </table>

      <div class="modal-actions">
        <button @click="versionsFor = null">关闭</button>
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
</style>
