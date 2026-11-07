<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { can } from '../store'

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

const editing = ref<any>(null)
const creating = ref(false)
const form = ref({ slug: '', name: '', description: '', payload: '{}', secret: '', enabled: false, activation: 'optional' })

const grantFor = ref<any>(null)
const grantType = ref('all')
const grantRoleId = ref('')
const grantUserId = ref('')

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
  form.value = { slug: '', name: '', description: '', payload: '{}', secret: '', enabled: false, activation: 'optional' }
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
  }
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
      await api.post('/api/admin/resources', {
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
  grantType.value = 'all'
  grantRoleId.value = roles.value[0]?.id || ''
  grantUserId.value = members.value[0]?.id || ''
  try {
    const res = await api.get(`/api/admin/resources/${row.id}/grants`)
    const first = res.items?.[0]
    if (first) {
      grantType.value = first.type
      if (first.type === 'role') grantRoleId.value = first.id
      if (first.type === 'user') grantUserId.value = first.id
    }
  } catch {
    /* 没有授权记录时保持默认 */
  }
}

async function saveGrants() {
  const items =
    grantType.value === 'all'
      ? [{ type: 'all', id: '' }]
      : grantType.value === 'role'
      ? [{ type: 'role', id: grantRoleId.value }]
      : [{ type: 'user', id: grantUserId.value }]
  try {
    await api.put(`/api/admin/resources/${grantFor.value.id}/grants`, { items })
    notice.value = '授权已保存'
    grantFor.value = null
  } catch (e) {
    error.value = errText(e)
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

function humanSize(bytes: number) {
  if (!bytes) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function when(ts: number) {
  return new Date(ts * 1000).toLocaleString('zh-CN')
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

  <div class="panel" style="margin-top: 16px">
    <div v-if="loading" class="empty">加载中…</div>
    <div v-else-if="!items.length" class="empty">这一类还没有资源</div>
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
        <tr v-for="row in items" :key="row.id">
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
        <input type="radio" value="all" v-model="grantType" style="width: auto" />
        <span>全员可见</span>
      </label>

      <label class="row" style="gap: 8px">
        <input type="radio" value="role" v-model="grantType" style="width: auto" />
        <span>按角色</span>
      </label>
      <select v-if="grantType === 'role'" v-model="grantRoleId">
        <option v-for="role in roles" :key="role.id" :value="role.id">
          {{ role.name }}（{{ role.key }}）
        </option>
      </select>

      <label class="row" style="gap: 8px">
        <input type="radio" value="user" v-model="grantType" style="width: auto" />
        <span>按成员</span>
      </label>
      <select v-if="grantType === 'user'" v-model="grantUserId">
        <option v-for="m in members" :key="m.id" :value="m.id">
          {{ m.displayName || m.username }}
        </option>
      </select>

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
