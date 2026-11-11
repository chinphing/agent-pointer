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

const items = ref<Snapshot[]>([])
const total = ref(0)
const limit = ref(50)
const userId = ref('')
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
    const params = new URLSearchParams({ limit: String(limit.value) })
    if (userId.value.trim()) params.set('userId', userId.value.trim())
    const res = await api.get(`/api/admin/conversations?${params.toString()}`)
    items.value = res.items
    total.value = res.total
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
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

onMounted(load)
</script>

<template>
  <div class="spread">
    <div>
      <p class="muted small" style="margin: 0">{{ t('conversations.subtitle') }}</p>
    </div>
    <div class="row">
      <input
        v-model="userId"
        :placeholder="t('conversations.filterUser')"
        style="width: 200px"
        @keyup.enter="load"
      />
      <select v-model.number="limit" style="width: 110px" @change="load">
        <option :value="50">{{ t('conversations.recent', { n: 50 }) }}</option>
        <option :value="100">{{ t('conversations.recent', { n: 100 }) }}</option>
        <option :value="200">{{ t('conversations.recent', { n: 200 }) }}</option>
      </select>
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

  <p v-if="!loading && items.length" class="muted small" style="margin-top: 10px">
    {{ t('conversations.count', { n: items.length, total }) }}
  </p>

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
