<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api } from '../../api'
import { when } from '../../lib/format'
import { ledgerKindLabel } from '../../lib/ledger'
import { widgetsFor } from '../../modules/registry'

const { t } = useI18n()

/** 私有仓注入的卡片（充值、订单历史）；开源仓这里是空数组。 */
const billingWidgets = widgetsFor('billing').map((w) => ({
  id: w.id,
  component: defineAsyncComponent(w.component as () => Promise<any>),
}))

interface LedgerRow {
  kind: string
  amountYuan: string
  balanceYuan: string
  model: string
  note: string
  createdAt: number
}

const PAGE_SIZE = 20

const loading = ref(true)
const error = ref('')
const balanceYuan = ref('0.00')
const rows = ref<LedgerRow[]>([])
const total = ref(0)
const offset = ref(0)

const page = computed(() => Math.floor(offset.value / PAGE_SIZE) + 1)
const pages = computed(() => Math.max(1, Math.ceil(total.value / PAGE_SIZE)))
const hasPrev = computed(() => offset.value > 0)
const hasNext = computed(() => offset.value + PAGE_SIZE < total.value)

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}


async function load() {
  loading.value = true
  error.value = ''
  try {
    const [wallet, ledger] = await Promise.all([
      api.get('/api/admin/my-wallet'),
      api.get(`/api/admin/my-wallet/ledger?limit=${PAGE_SIZE}&offset=${offset.value}`),
    ])
    balanceYuan.value = wallet.balanceYuan ?? '0.00'
    rows.value = ledger.items ?? []
    total.value = ledger.total ?? 0
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

function go(delta: number) {
  const next = offset.value + delta * PAGE_SIZE
  if (next < 0) return
  offset.value = next
  void load()
}

onMounted(load)
</script>

<template>
  <div class="me-page">
    <div v-if="error" class="err">{{ error }}</div>

    <section class="me-card">
      <div class="me-balance-label">{{ t('me.balance') }}</div>
      <div class="me-balance">¥{{ balanceYuan }}</div>
    </section>

    <section class="me-card">
      <div class="me-card-head">
        <div>
          <div class="me-card-title">{{ t('me.ledger') }}</div>
          <div class="me-card-sub">{{ t('me.ledgerHint') }}</div>
        </div>
        <span v-if="!loading" class="muted small">
          {{ t('me.totalRecords', { n: total }) }}
        </span>
      </div>

      <div v-if="loading" class="empty">{{ t('common.loading') }}</div>
      <div v-else-if="!rows.length" class="empty me-empty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden>
          <path d="M5 3.75h9L19 8.5v11.75H5z" stroke-linejoin="round" />
          <path d="M14 3.75V8.5h5M8 12.5h8M8 16h5" stroke-linecap="round" />
        </svg>
        <p>{{ t('me.emptyLedger') }}</p>
      </div>
      <table v-else class="table me-table">
        <thead>
          <tr>
            <th>{{ t('me.time') }}</th>
            <th>{{ t('me.kind') }}</th>
            <th class="num">{{ t('me.amount') }}</th>
            <th class="num">{{ t('me.balanceAfter') }}</th>
            <th>{{ t('me.note') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, i) in rows" :key="i">
            <td class="mono small">{{ when(row.createdAt) }}</td>
            <td>{{ ledgerKindLabel(row.kind, t) }}</td>
            <td class="mono num">¥{{ row.amountYuan }}</td>
            <td class="mono num">¥{{ row.balanceYuan }}</td>
            <td class="small">{{ row.model || row.note }}</td>
          </tr>
        </tbody>
      </table>

      <div v-if="!loading && total > PAGE_SIZE" class="me-pager">
        <button class="btn" :disabled="!hasPrev" @click="go(-1)">{{ t('me.prev') }}</button>
        <span class="muted small">{{ t('me.pageOf', { page, pages }) }}</span>
        <button class="btn" :disabled="!hasNext" @click="go(1)">{{ t('me.next') }}</button>
      </div>
    </section>

    <!-- 私有仓注入的卡片（充值、订单历史）；开源仓没有模块，这里不渲染 -->
    <section v-for="w in billingWidgets" :key="w.id" class="me-card">
      <component :is="w.component" />
    </section>
  </div>
</template>
