<script setup lang="ts">
/**
 * 会话：查看客户端上报上来的对话快照。
 *
 * 这是敏感数据（对话里可能有患者信息），所以：
 * - 只有带 `conversation:read` 的角色能进（内置 viewer 默认没有）
 * - 每次打开详情，服务端都会写一条 `conversation_view` 审计
 */
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { when } from '../lib/format'

const { t } = useI18n()

interface Snapshot {
  id: string
  userId: string
  userName: string
  conversationId: string
  agentInstanceId: string
  agentRoleId: string | null
  model: string
  promptTokens: number
  completionTokens: number
  messageCount: number
  preview: string
  sizeBytes: number
  createdAt: number
}

interface Message {
  id?: string
  role: string
  content: string
  tool_name?: string
  agent_name?: string
  created_at?: string
  images?: string[]
}

interface Member {
  id: string
  displayName: string
  username: string
}

const items = ref<Snapshot[]>([])
const total = ref(0)
const totalCapped = ref(false)
const limit = ref(50)
const page = ref(0) // 从 0 开始
const userId = ref('')
const fromDate = ref('')
const toDate = ref('')
const members = ref<Member[]>([])
const loading = ref(true)
const error = ref('')

const detail = ref<Snapshot | null>(null)
const messages = ref<Message[]>([])
const detailLoading = ref(false)
const detailError = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    const params = new URLSearchParams({
      limit: String(limit.value),
      offset: String(page.value * limit.value),
    })
    if (userId.value) params.set('userId', userId.value)
    if (fromDate.value) params.set('from', fromDate.value)
    if (toDate.value) params.set('to', toDate.value)
    const res = await api.get(`/api/admin/conversations?${params.toString()}`)
    items.value = res.items
    total.value = res.total
    totalCapped.value = !!res.totalCapped
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

/** 改了筛选条件就回到第一页，否则会停在一个空页上。 */
function applyFilter() {
  page.value = 0
  load()
}

function resetFilter() {
  userId.value = ''
  fromDate.value = ''
  toDate.value = ''
  page.value = 0
  load()
}

function prevPage() {
  if (page.value === 0) return
  page.value -= 1
  load()
}

/** 本页没满就说明到底了；总数封顶时不能只看 total。 */
function hasNext() {
  return items.value.length >= limit.value
}

function nextPage() {
  if (!hasNext()) return
  page.value += 1
  load()
}

/** 成员下拉：没有 user:read 权限时降级成手输 ID。 */
async function loadMembers() {
  try {
    const res = await api.get('/api/admin/users')
    members.value = (res.items ?? []).map((u: Member) => ({
      id: u.id,
      displayName: u.displayName || u.username || u.id,
      username: u.username,
    }))
  } catch {
    members.value = []
  }
}

async function open(row: Snapshot) {
  detail.value = row
  messages.value = []
  detailLoading.value = true
  detailError.value = ''
  try {
    const res = await api.get(`/api/admin/conversations/${row.id}`)
    messages.value = res.messages ?? []
  } catch (e) {
    detailError.value = errText(e)
  } finally {
    detailLoading.value = false
  }
}

function close() {
  detail.value = null
  messages.value = []
}

function roleLabel(role: string) {
  return t(`conversations.role.${role}`)
}

/** 气泡里的时间是 RFC3339 字符串，转成跟列表一致的本地时间。 */
function msgTime(raw?: string) {
  if (!raw) return ''
  const ts = Date.parse(raw)
  return Number.isNaN(ts) ? raw : when(Math.floor(ts / 1000))
}

function sizeText(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

onMounted(() => {
  loadMembers()
  load()
})
</script>

<template>
  <div class="spread">
    <div>
      <p class="muted small" style="margin: 0">{{ t('conversations.subtitle') }}</p>
    </div>
    <div class="row">
      <select v-if="members.length" v-model="userId" style="width: 170px" @change="applyFilter">
        <option value="">{{ t('conversations.allMembers') }}</option>
        <option v-for="m in members" :key="m.id" :value="m.id">{{ m.displayName }}</option>
      </select>
      <input
        v-else
        v-model="userId"
        :placeholder="t('conversations.filterUser')"
        style="width: 180px"
        @keyup.enter="applyFilter"
      />
      <input
        v-model="fromDate"
        type="date"
        :title="t('conversations.from')"
        style="width: 150px"
        @change="applyFilter"
      />
      <span class="muted small">—</span>
      <input
        v-model="toDate"
        type="date"
        :title="t('conversations.to')"
        style="width: 150px"
        @change="applyFilter"
      />
      <select v-model.number="limit" style="width: 110px" @change="applyFilter">
        <option :value="50">{{ t('conversations.pageSize', { n: 50 }) }}</option>
        <option :value="100">{{ t('conversations.pageSize', { n: 100 }) }}</option>
        <option :value="200">{{ t('conversations.pageSize', { n: 200 }) }}</option>
      </select>
      <button class="ghost sm" @click="resetFilter">{{ t('conversations.reset') }}</button>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>

  <div class="panel" style="margin-top: 16px">
    <div v-if="loading" class="empty">{{ t('common.loading') }}</div>
    <div v-else-if="!items.length" class="empty">{{ t('conversations.empty') }}</div>
    <table v-else>
      <thead>
        <tr>
          <th style="width: 160px">{{ t('conversations.time') }}</th>
          <th style="width: 120px">{{ t('conversations.user') }}</th>
          <th>{{ t('conversations.preview') }}</th>
          <th style="width: 80px">{{ t('conversations.messages') }}</th>
          <th style="width: 130px">{{ t('conversations.model') }}</th>
          <th style="width: 90px">{{ t('conversations.size') }}</th>
          <th style="width: 70px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in items" :key="row.id">
          <td class="small">{{ when(row.createdAt) }}</td>
          <td class="small">{{ row.userName }}</td>
          <td class="small">{{ row.preview || '—' }}</td>
          <td class="small">{{ row.messageCount }}</td>
          <td class="small mono">{{ row.model || '—' }}</td>
          <td class="small">{{ sizeText(row.sizeBytes) }}</td>
          <td>
            <button class="ghost sm" @click="open(row)">{{ t('conversations.view') }}</button>
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <div
    v-if="!loading && (items.length || page > 0)"
    class="spread"
    style="margin-top: 10px; align-items: center"
  >
    <p class="muted small" style="margin: 0">
      {{
        totalCapped
          ? t('conversations.countCapped', { n: items.length, total })
          : t('conversations.count', { n: items.length, total })
      }}
    </p>
    <div class="row">
      <button class="ghost sm" :disabled="page === 0" @click="prevPage">
        {{ t('conversations.prev') }}
      </button>
      <span class="muted small">{{ t('conversations.pageNo', { n: page + 1 }) }}</span>
      <button class="ghost sm" :disabled="!hasNext()" @click="nextPage">
        {{ t('conversations.next') }}
      </button>
    </div>
  </div>

  <!-- 详情：抽屉 -->
  <div v-if="detail" class="drawer-mask" @click.self="close">
    <div class="drawer">
      <div class="spread" style="align-items: flex-start">
        <div>
          <div style="font-weight: 600">{{ t('conversations.detailTitle') }}</div>
          <div class="muted small" style="margin-top: 4px">
            {{ detail.userName }} · {{ when(detail.createdAt) }} ·
            <span class="mono">{{ detail.model || '—' }}</span>
          </div>
          <div class="muted small mono" style="margin-top: 2px">
            {{ detail.conversationId }}
          </div>
        </div>
        <button class="ghost sm" @click="close">{{ t('common.close') }}</button>
      </div>

      <div v-if="detailLoading" class="empty">{{ t('common.loading') }}</div>
      <div v-else-if="detailError" class="err">{{ detailError }}</div>
      <div v-else-if="!messages.length" class="empty">{{ t('conversations.noMessages') }}</div>
      <div v-else class="thread">
        <div v-for="(msg, i) in messages" :key="msg.id || i" :class="['bubble', msg.role]">
          <div class="bubble-head">
            <span class="tag plain">{{ roleLabel(msg.role) }}</span>
            <span v-if="msg.tool_name" class="mono small">{{ msg.tool_name }}</span>
            <span v-if="msg.agent_name" class="muted small">{{ msg.agent_name }}</span>
            <span class="muted small">{{ msgTime(msg.created_at) }}</span>
          </div>
          <div class="bubble-body">{{ msg.content }}</div>
          <div v-if="msg.images && msg.images.length" class="muted small">
            {{ msg.images.join(' ') }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.drawer-mask {
  position: fixed;
  inset: 0;
  background: rgba(15, 17, 21, 0.32);
  display: flex;
  justify-content: flex-end;
  z-index: 40;
}
.drawer {
  width: min(720px, 92vw);
  height: 100%;
  background: #fff;
  padding: 20px 22px;
  overflow: auto;
  box-shadow: -8px 0 24px rgba(15, 17, 21, 0.12);
}
.thread {
  margin-top: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.bubble {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 10px 12px;
  background: #fafbfc;
}
.bubble.user {
  background: #f2f7ff;
  border-color: #cfe0f7;
}
.bubble.tool {
  background: #fbfaf4;
  border-color: #e8e2c8;
}
.bubble-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.bubble-body {
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 13px;
  line-height: 1.6;
}
</style>
