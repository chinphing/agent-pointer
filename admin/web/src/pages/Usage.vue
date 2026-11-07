<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { can } from '../store'

const from = ref('')
const to = ref('')
const data = ref<any>(null)
const loading = ref(true)
const error = ref('')
const notice = ref('')

const quotaUser = ref('')
const quotaLimit = ref<number | ''>('')
const quotaAction = ref('warn')
const quotaState = ref<any>(null)

function fmt(n: number) {
  return Number(n || 0).toLocaleString('zh-CN')
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const params = new URLSearchParams()
    if (from.value) params.set('from', from.value)
    if (to.value) params.set('to', to.value)
    const qs = params.toString()
    data.value = await api.get(`/api/admin/usage${qs ? `?${qs}` : ''}`)
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

async function loadQuota() {
  if (!quotaUser.value.trim()) return
  error.value = ''
  try {
    const q = await api.get(`/api/admin/usage/quota/${encodeURIComponent(quotaUser.value.trim())}`)
    quotaState.value = q
    quotaLimit.value = q.monthlyTokenLimit ?? ''
    quotaAction.value = q.overAction
  } catch (e) {
    quotaState.value = null
    error.value = errText(e)
  }
}

async function saveQuota() {
  if (!quotaUser.value.trim()) return
  error.value = ''
  try {
    await api.put(`/api/admin/usage/quota/${encodeURIComponent(quotaUser.value.trim())}`, {
      monthlyTokenLimit: quotaLimit.value === '' ? null : Number(quotaLimit.value),
      overAction: quotaAction.value,
    })
    notice.value = '配额已保存'
    await loadQuota()
  } catch (e) {
    error.value = errText(e)
  }
}

onMounted(load)
</script>

<template>
  <div class="spread">
    <div>
      <h1>用量</h1>
      <p class="muted small" style="margin: 4px 0 0">数据来自客户端/服务端上报的 token 用量。</p>
    </div>
    <div class="row">
      <input v-model="from" type="date" style="width: 150px" />
      <span class="muted">至</span>
      <input v-model="to" type="date" style="width: 150px" />
      <button @click="load">查询</button>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>

  <div v-if="loading" class="empty">加载中…</div>

  <template v-else-if="data">
    <div class="panel" style="margin-top: 16px">
      <div class="spread">
        <h2>合计</h2>
        <div class="small muted">{{ data.from }} ~ {{ data.to }}</div>
      </div>
      <div class="grid" style="margin-top: 12px">
        <div>
          <div class="muted small">输入 tokens</div>
          <div class="big">{{ fmt(data.totals.promptTokens) }}</div>
        </div>
        <div>
          <div class="muted small">输出 tokens</div>
          <div class="big">{{ fmt(data.totals.completionTokens) }}</div>
        </div>
        <div>
          <div class="muted small">总计</div>
          <div class="big">{{ fmt(data.totals.total) }}</div>
        </div>
      </div>
    </div>

    <div class="panel">
      <h2>按成员</h2>
      <div v-if="!data.byUser.length" class="empty">没有数据</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>成员 ID</th><th class="num">输入</th><th class="num">输出</th><th class="num">合计</th></tr>
        </thead>
        <tbody>
          <tr v-for="row in data.byUser" :key="row.key">
            <td class="mono">{{ row.key }}</td>
            <td class="num">{{ fmt(row.promptTokens) }}</td>
            <td class="num">{{ fmt(row.completionTokens) }}</td>
            <td class="num">{{ fmt(row.total) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="panel">
      <h2>按模型</h2>
      <div v-if="!data.byModel.length" class="empty">没有数据</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>模型</th><th class="num">输入</th><th class="num">输出</th><th class="num">合计</th></tr>
        </thead>
        <tbody>
          <tr v-for="row in data.byModel" :key="row.key">
            <td class="mono">{{ row.key || '（未标注）' }}</td>
            <td class="num">{{ fmt(row.promptTokens) }}</td>
            <td class="num">{{ fmt(row.completionTokens) }}</td>
            <td class="num">{{ fmt(row.total) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="panel">
      <h2>按日期</h2>
      <div v-if="!data.byDay.length" class="empty">没有数据</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>日期</th><th class="num">输入</th><th class="num">输出</th><th class="num">合计</th></tr>
        </thead>
        <tbody>
          <tr v-for="row in data.byDay" :key="row.key">
            <td class="mono">{{ row.key }}</td>
            <td class="num">{{ fmt(row.promptTokens) }}</td>
            <td class="num">{{ fmt(row.completionTokens) }}</td>
            <td class="num">{{ fmt(row.total) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="can('usage:read')" class="panel">
      <h2>配额</h2>
      <p class="muted small">按月累计 token。warn 只提示，block 会让客户端停用该账号的模型调用。</p>
      <div class="row" style="margin-top: 12px">
        <input v-model="quotaUser" placeholder="成员 ID" class="mono" style="max-width: 340px" />
        <button @click="loadQuota">查询</button>
      </div>

      <div v-if="quotaState" class="quota-box">
        <div class="small muted">
          本月已用 <strong>{{ fmt(quotaState.used) }}</strong>
          <span v-if="quotaState.monthlyTokenLimit"> / 上限 {{ fmt(quotaState.monthlyTokenLimit) }}</span>
          <span v-if="quotaState.blocks" class="tag danger" style="margin-left: 8px">已拦截</span>
          <span v-else-if="quotaState.exceeded" class="tag warn" style="margin-left: 8px">已超限（仅提示）</span>
        </div>
        <div class="row" style="margin-top: 12px">
          <div style="width: 200px">
            <label>每月上限（留空=不限）</label>
            <input v-model.number="quotaLimit" type="number" min="0" placeholder="不限" />
          </div>
          <div style="width: 160px">
            <label>超限动作</label>
            <select v-model="quotaAction">
              <option value="warn">warn（提示）</option>
              <option value="block">block（拦截）</option>
            </select>
          </div>
          <button class="primary" style="margin-top: 18px" :disabled="!can('usage:write')" @click="saveQuota">
            保存
          </button>
        </div>
      </div>
    </div>
  </template>
</template>

<style scoped>
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
.big { font-size: 24px; font-weight: 600; }
.quota-box { margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); }
</style>
