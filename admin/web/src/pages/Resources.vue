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
const form = ref({ slug: '', name: '', description: '', payload: '{}', secret: '', enabled: false })

const grantFor = ref<any>(null)
const grantType = ref('all')
const grantRoleId = ref('')
const grantUserId = ref('')

const revealed = ref<{ name: string; secret: string } | null>(null)

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
  form.value = { slug: '', name: '', description: '', payload: '{}', secret: '', enabled: false }
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
      })
      notice.value = '资源已创建'
    } else {
      const body: any = {
        name: form.value.name.trim(),
        description: form.value.description.trim(),
        payload,
        enabled: form.value.enabled,
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
</template>
