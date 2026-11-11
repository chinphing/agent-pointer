<script setup lang="ts">
/**
 * 会话：查看客户端上报上来的对话。
 *
 * 这是敏感数据（对话里可能有患者信息），所以：
 * - 只有带 `conversation:read` 的角色能进（内置 viewer 默认没有）
 * - 每展开一轮，服务端都会写一条 `conversation_view` 审计
 *
 * 三级读取，越往后越贵：
 * 1. 列表 —— 一个会话一行
 * 2. 抽屉 —— tab 切主会话/子会话，轮次只显示摘要（不读 zip）
 * 3. 展开某一轮 —— 才去读那一个 zip
 */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { epochSeconds, when } from '../lib/format'

const { t } = useI18n()

/** 列表一行 = 一个会话（主会话 + 若干子会话合成）。 */
interface Conversation {
  conversationId: string
  userId: string
  userName: string
  title: string
  model: string
  threadCount: number
  turnCount: number
  messageCount: number
  firstAt: number
  lastAt: number
}

/** 一个 agent 实例的会话，抽屉里的一个 tab。 */
interface Thread {
  id: string
  agentInstanceId: string
  agentRoleId: string | null
  agentName: string | null
  isMain: boolean
  turnCount: number
  messageCount: number
  firstAt: number
  lastAt: number
}

/** 一轮：只有摘要，展开才读正文。 */
interface Turn {
  id: string
  seq: number
  turnAt: number
  preview: string
  messageCount: number
  sizeBytes: number
}

interface Message {
  id?: string
  role: string
  content: string
  tool_name?: string
  agent_name?: string
  created_at?: number | string
  images?: string[]
}

interface Member {
  id: string
  displayName: string
  username: string
}

/** 一轮的消息条数不少，但折叠行很矮，20 行刚好一屏多一点。 */
const TURN_PAGE = 20

const items = ref<Conversation[]>([])
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

const detail = ref<Conversation | null>(null)
const threads = ref<Thread[]>([])
const activeThreadId = ref('')
const turns = ref<Turn[]>([])
const turnsTotal = ref(0)
const turnsLoading = ref(false)
const turnsError = ref('')
/** 已展开的轮次 → 消息数组。展开过就缓存，收起不清，避免反复读盘。 */
const expanded = ref<Record<string, Message[]>>({})
const expanding = ref<Record<string, boolean>>({})

const activeThread = computed(() => threads.value.find((x) => x.id === activeThreadId.value))
const hasMoreTurns = computed(() => turns.value.length < turnsTotal.value)

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

function resetDrawer() {
  threads.value = []
  activeThreadId.value = ''
  turns.value = []
  turnsTotal.value = 0
  expanded.value = {}
  expanding.value = {}
  turnsError.value = ''
}

async function open(row: Conversation) {
  detail.value = row
  resetDrawer()
  turnsLoading.value = true
  try {
    const res = await api.get(
      `/api/admin/conversations/${encodeURIComponent(row.conversationId)}/tabs`,
    )
    threads.value = res.items ?? []
    if (threads.value.length) {
      await selectThread(threads.value[0].id)
    }
  } catch (e) {
    turnsError.value = errText(e)
  } finally {
    turnsLoading.value = false
  }
}

async function selectThread(id: string) {
  activeThreadId.value = id
  turns.value = []
  turnsTotal.value = 0
  expanded.value = {}
  await loadTurns(false)
}

/** `append` 为 true 时往后续接更早的轮次（新的在前，所以是往后加）。 */
async function loadTurns(append: boolean) {
  const threadId = activeThreadId.value
  if (!threadId) return
  turnsLoading.value = true
  turnsError.value = ''
  try {
    const offset = append ? turns.value.length : 0
    const res = await api.get(
      `/api/admin/threads/${encodeURIComponent(threadId)}/turns?limit=${TURN_PAGE}&offset=${offset}`,
    )
    const pageRows: Turn[] = res.items ?? []
    turns.value = append ? [...turns.value, ...pageRows] : pageRows
    turnsTotal.value = res.total ?? 0
  } catch (e) {
    turnsError.value = errText(e)
  } finally {
    turnsLoading.value = false
  }
}

async function toggleTurn(turn: Turn) {
  if (expanded.value[turn.id]) {
    const next = { ...expanded.value }
    delete next[turn.id]
    expanded.value = next
    return
  }
  if (expanding.value[turn.id]) return
  expanding.value = { ...expanding.value, [turn.id]: true }
  try {
    const res = await api.get(`/api/admin/turns/${encodeURIComponent(turn.id)}`)
    expanded.value = { ...expanded.value, [turn.id]: res.messages ?? [] }
  } catch (e) {
    turnsError.value = errText(e)
  } finally {
    const next = { ...expanding.value }
    delete next[turn.id]
    expanding.value = next
  }
}

function close() {
  detail.value = null
  resetDrawer()
}

function roleLabel(role: string) {
  return t(`conversations.role.${role}`)
}

/** 气泡里的时间是 epoch（秒或毫秒）或字符串，统一显示成本地时间。 */
function msgTime(raw?: number | string) {
  const ts = epochSeconds(raw)
  // 解析不了才退回原值：宁可显示原始串，也不要显示一个假时间
  return ts === null ? String(raw ?? '') : when(ts)
}

function threadLabel(th: Thread) {
  if (th.isMain) return t('conversations.mainThread')
  return th.agentName || th.agentRoleId || t('conversations.subThread')
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
          <th style="width: 150px">{{ t('conversations.time') }}</th>
          <th style="width: 110px">{{ t('conversations.user') }}</th>
          <th>{{ t('conversations.preview') }}</th>
          <th style="width: 70px">{{ t('conversations.turns') }}</th>
          <th style="width: 70px">{{ t('conversations.messages') }}</th>
          <th style="width: 130px">{{ t('conversations.model') }}</th>
          <th style="width: 70px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in items" :key="row.conversationId">
          <td class="small">{{ when(row.lastAt) }}</td>
          <td class="small">{{ row.userName }}</td>
          <td class="small">{{ row.title || '—' }}</td>
          <td class="small">{{ row.turnCount }}</td>
          <td class="small">{{ row.messageCount }}</td>
          <td class="small mono">{{ row.model || '—' }}</td>
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
            {{ detail.userName }} · {{ when(detail.lastAt) }} ·
            <span class="mono">{{ detail.model || '—' }}</span>
          </div>
          <div class="muted small mono" style="margin-top: 2px">
            {{ detail.conversationId }}
          </div>
        </div>
        <button class="ghost sm" @click="close">{{ t('common.close') }}</button>
      </div>

      <!-- 主会话 / 子会话。只有一个时不显示 tab 条，省得占地方 -->
      <div v-if="threads.length > 1" class="thread-tabs">
        <button
          v-for="th in threads"
          :key="th.id"
          :class="['thread-tab', { active: th.id === activeThreadId }]"
          @click="selectThread(th.id)"
        >
          <span>{{ threadLabel(th) }}</span>
          <span class="thread-tab-count">{{ t('conversations.turnCount', { n: th.turnCount }) }}</span>
        </button>
      </div>

      <div v-if="turnsError" class="err" style="margin-top: 12px">{{ turnsError }}</div>
      <div v-else-if="turnsLoading && !turns.length" class="empty">
        {{ t('common.loading') }}
      </div>
      <div v-else-if="!turns.length" class="empty">{{ t('conversations.noMessages') }}</div>
      <div v-else class="turns">
        <div v-for="turn in turns" :key="turn.id" class="turn">
          <button class="turn-head" @click="toggleTurn(turn)">
            <span class="tag plain">{{ t('conversations.turnLabel', { n: turn.seq }) }}</span>
            <span class="muted small">{{ when(turn.turnAt) }}</span>
            <span class="muted small">
              {{ t('conversations.turnMessages', { n: turn.messageCount }) }}
            </span>
            <span class="turn-preview">{{ turn.preview || '—' }}</span>
            <span class="turn-caret">{{ expanded[turn.id] ? '▾' : '▸' }}</span>
          </button>

          <div v-if="expanding[turn.id]" class="muted small" style="padding: 10px 12px">
            {{ t('common.loading') }}
          </div>
          <div v-else-if="expanded[turn.id]" class="thread">
            <div
              v-for="(msg, i) in expanded[turn.id]"
              :key="msg.id || i"
              :class="['bubble', msg.role]"
            >
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

        <button
          v-if="hasMoreTurns"
          class="ghost sm"
          style="margin-top: 10px; align-self: center"
          :disabled="turnsLoading"
          @click="loadTurns(true)"
        >
          {{
            t('conversations.loadMore', {
              n: Math.min(TURN_PAGE, turnsTotal - turns.length),
            })
          }}
        </button>
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
.thread-tabs {
  display: flex;
  gap: 6px;
  margin-top: 14px;
  flex-wrap: wrap;
  border-bottom: 1px solid var(--line);
  padding-bottom: 8px;
}
.thread-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--line);
  background: #fff;
  border-radius: 8px;
  padding: 5px 10px;
  font-size: 12px;
  cursor: pointer;
  color: #0f1115;
}
.thread-tab.active {
  background: #f2f7ff;
  border-color: #cfe0f7;
  color: #0f1115;
  font-weight: 600;
}
.thread-tab-count {
  color: #6b7280;
  font-weight: 400;
}
.turns {
  margin-top: 12px;
  display: flex;
  flex-direction: column;
}
.turn {
  border-bottom: 1px solid var(--line);
}
.turn-head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  background: none;
  border: 0;
  padding: 10px 2px;
  cursor: pointer;
  text-align: left;
  color: inherit;
}
.turn-head:hover {
  background: #fafbfc;
}
.turn-preview {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.turn-caret {
  color: #9ca3af;
  font-size: 12px;
}
.thread {
  padding: 4px 2px 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  /* 气泡按内容收缩，不要拉满整行——短回复铺满一条会显得很空 */
  align-items: flex-start;
}
.bubble {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 10px 12px;
  background: #fafbfc;
  /* 气泡不铺满整行，否则左右对齐看不出区别 */
  max-width: 86%;
}
.bubble.user {
  background: #f2f7ff;
  border-color: #cfe0f7;
  /* 用户说的话靠右，和助手的靠左区分开 */
  align-self: flex-end;
}
.bubble.user .bubble-head {
  justify-content: flex-end;
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
