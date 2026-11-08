<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useNotice } from '../lib/notice'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { fmtCount as fmt } from '../lib/format'
import { can } from '../store'

const { t } = useI18n()
const from = ref('')
const to = ref('')
const data = ref<any>(null)
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()

const quotaUser = ref('')
const quotaLimit = ref<number | ''>('')
const quotaAction = ref('warn')
const quotaState = ref<any>(null)

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
    flashNotice(t('usage.quotaSaved'))
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
      <p class="muted small" style="margin: 0">{{ t('usage.subtitle') }}</p>
    </div>
    <div class="row">
      <input v-model="from" type="date" style="width: 150px" />
      <span class="muted">{{ t('usage.to') }}</span>
      <input v-model="to" type="date" style="width: 150px" />
      <button @click="load">{{ t('usage.query') }}</button>
    </div>
  </div>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-if="notice" class="okmsg" style="margin-top: 16px">{{ notice }}</div>

  <div v-if="loading" class="empty">{{ t('common.loading') }}</div>

  <template v-else-if="data">
    <div class="panel" style="margin-top: 16px">
      <div class="spread">
        <h2>{{ t('usage.total') }}</h2>
        <div class="small muted">{{ data.from }} ~ {{ data.to }}</div>
      </div>
      <div class="grid" style="margin-top: 12px">
        <div>
          <div class="muted small">{{ t('usage.inputTokens') }}</div>
          <div class="big">{{ fmt(data.totals.promptTokens) }}</div>
        </div>
        <div>
          <div class="muted small">{{ t('usage.outputTokens') }}</div>
          <div class="big">{{ fmt(data.totals.completionTokens) }}</div>
        </div>
        <div>
          <div class="muted small">{{ t('usage.total') }}</div>
          <div class="big">{{ fmt(data.totals.total) }}</div>
        </div>
      </div>
    </div>

    <div class="panel">
      <h2>{{ t('usage.byMember') }}</h2>
      <div v-if="!data.byUser.length" class="empty">{{ t('usage.noData') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>{{ t('usage.memberId') }}</th><th class="num">{{ t('usage.input') }}</th><th class="num">{{ t('usage.output') }}</th><th class="num">{{ t('usage.total') }}</th></tr>
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
      <h2>{{ t('usage.byModel') }}</h2>
      <div v-if="!data.byModel.length" class="empty">{{ t('usage.noData') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>{{ t('usage.model') }}</th><th class="num">{{ t('usage.input') }}</th><th class="num">{{ t('usage.output') }}</th><th class="num">{{ t('usage.total') }}</th></tr>
        </thead>
        <tbody>
          <tr v-for="row in data.byModel" :key="row.key">
            <td class="mono">{{ row.key || t('usage.unlabeled') }}</td>
            <td class="num">{{ fmt(row.promptTokens) }}</td>
            <td class="num">{{ fmt(row.completionTokens) }}</td>
            <td class="num">{{ fmt(row.total) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="panel">
      <h2>{{ t('usage.byDate') }}</h2>
      <div v-if="!data.byDay.length" class="empty">{{ t('usage.noData') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>{{ t('usage.date') }}</th><th class="num">{{ t('usage.input') }}</th><th class="num">{{ t('usage.output') }}</th><th class="num">{{ t('usage.total') }}</th></tr>
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
      <h2>{{ t('usage.quota') }}</h2>
      <p class="muted small">{{ t('usage.quotaHint') }}</p>
      <div class="row" style="margin-top: 12px">
        <input v-model="quotaUser" :placeholder="t('usage.memberId')" class="mono" style="max-width: 340px" />
        <button @click="loadQuota">{{ t('usage.query') }}</button>
      </div>

      <div v-if="quotaState" class="quota-box">
        <div class="small muted">
          {{ t('usage.usedThisMonth') }} <strong>{{ fmt(quotaState.used) }}</strong>
          <span v-if="quotaState.monthlyTokenLimit">
            {{ t('usage.limitOf', { n: fmt(quotaState.monthlyTokenLimit) }) }}
          </span>
          <span v-if="quotaState.blocks" class="tag danger" style="margin-left: 8px">{{ t('usage.blocked') }}</span>
          <span v-else-if="quotaState.exceeded" class="tag warn" style="margin-left: 8px">{{ t('usage.overLimit') }}</span>
        </div>
        <div class="row" style="margin-top: 12px">
          <div style="width: 200px">
            <label>{{ t('usage.monthlyLimit') }}</label>
            <input v-model.number="quotaLimit" type="number" min="0" :placeholder="t('usage.unlimited')" />
          </div>
          <div style="width: 160px">
            <label>{{ t('usage.overAction') }}</label>
            <select v-model="quotaAction">
              <option value="warn">{{ t('usage.actionWarn') }}</option>
              <option value="block">{{ t('usage.actionBlock') }}</option>
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
