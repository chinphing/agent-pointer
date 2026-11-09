<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { useNotice } from '../lib/notice'
import { can } from '../store'
import { parseRatioInput } from '../lib/ratioInput'

type RatioRow = {
  model: string
  configured: boolean
  ratio: number
}

const { t } = useI18n()
const rows = ref<RatioRow[]>([])
const baseYuan = ref('1')
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()
const savingModel = ref('')
const savingBase = ref(false)

/** 每行可编辑的费率，按模型名存。未配费率留空，不显示 0。 */
const draft = reactive<Record<string, string>>({})

const editable = computed(() => can('usage:write'))

function syncDraft(list: RatioRow[]) {
  for (const row of list) {
    draft[row.model] = row.configured ? String(row.ratio) : ''
  }
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    const res = await api.get('/api/admin/model-prices')
    rows.value = res.items || []
    baseYuan.value = String(res.baseYuanPerMb ?? 1)
    syncDraft(rows.value)
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

async function saveBase() {
  const parsed = parseRatioInput(baseYuan.value)
  if (!parsed.ok) {
    error.value = t('billing.baseInvalid')
    return
  }
  const yuan = parsed.value
  error.value = ''
  savingBase.value = true
  try {
    await api.put('/api/admin/model-prices/base', { baseYuanPerMb: yuan })
    flashNotice(t('billing.baseSaved'))
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    savingBase.value = false
  }
}

async function saveRow(row: RatioRow) {
  // `v-model` 绑在 `type="number"` 上时值是 Number，不是字符串——不能直接 `.trim()`。
  const parsed = parseRatioInput(draft[row.model])
  if (!parsed.ok) {
    error.value = parsed.reason === 'empty' ? t('billing.ratioRequired') : t('billing.ratioInvalid')
    return
  }
  const ratio = parsed.value
  error.value = ''
  savingModel.value = row.model
  try {
    await api.post('/api/admin/model-prices', { model: row.model, ratio })
    flashNotice(t('billing.saved', { model: row.model }))
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    savingModel.value = ''
  }
}

async function clearRow(row: RatioRow) {
  error.value = ''
  savingModel.value = row.model
  try {
    await api.del(`/api/admin/model-prices/${encodeURIComponent(row.model)}`)
    flashNotice(t('billing.cleared', { model: row.model }))
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    savingModel.value = ''
  }
}

onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <h2>{{ t('nav.billing') }}</h2>
      <p class="muted small">{{ t('billing.hint') }}</p>
    </div>

    <div v-if="notice" class="okmsg" @click="clearNotice">{{ notice }}</div>
    <div v-if="error" class="err">{{ error }}</div>

    <div class="panel">
      <h2>{{ t('billing.baseTitle') }}</h2>
      <p class="muted small">{{ t('billing.baseHint') }}</p>
      <div class="row" style="margin-top: 12px">
        <input
          v-model="baseYuan"
          class="num base"
          type="number"
          min="0"
          step="0.01"
          :disabled="!editable"
          aria-label="基准价"
        />
        <span class="muted small">{{ t('billing.baseUnit') }}</span>
        <button v-if="editable" class="tiny" :disabled="savingBase" @click="saveBase">
          {{ t('billing.save') }}
        </button>
      </div>
    </div>

    <div class="panel">
      <h2>{{ t('billing.listTitle') }}</h2>
      <p class="muted small">{{ t('billing.ratioHint') }}</p>

      <p v-if="loading" class="muted">{{ t('common.loading') }}</p>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr>
            <th>{{ t('billing.model') }}</th>
            <th class="num">{{ t('billing.ratio') }}</th>
            <th>{{ t('billing.status') }}</th>
            <th class="actions"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.model">
            <td class="mono">{{ row.model }}</td>
            <td class="num">
              <input
                v-if="editable"
                v-model="draft[row.model]"
                class="num ratio"
                type="number"
                min="0"
                step="0.0001"
                :placeholder="t('billing.unpricedPlaceholder')"
                :aria-label="`${row.model} ${t('billing.ratio')}`"
              />
              <span v-else class="mono">{{ row.ratio }}</span>
            </td>
            <td>
              <span v-if="row.configured" class="tag on">{{ t('billing.configured') }}</span>
              <span v-else class="tag off">{{ t('billing.unpriced') }}</span>
            </td>
            <td class="actions">
              <template v-if="editable">
                <button class="tiny" :disabled="savingModel === row.model" @click="saveRow(row)">
                  {{ t('billing.save') }}
                </button>
                <button
                  v-if="row.configured"
                  class="tiny"
                  :disabled="savingModel === row.model"
                  @click="clearRow(row)"
                >
                  {{ t('billing.clear') }}
                </button>
              </template>
            </td>
          </tr>
          <tr v-if="!rows.length">
            <td colspan="4" class="muted small">{{ t('billing.emptyCatalog') }}</td>
          </tr>
        </tbody>
      </table>

      <p class="muted small">{{ t('billing.pricingRule') }}</p>
    </div>
  </div>
</template>

<style scoped>
input.ratio {
  width: 110px;
  text-align: right;
}
input.base {
  width: 120px;
  text-align: right;
}
.tag {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid rgba(0, 0, 0, 0.12);
  font-size: 12px;
  color: rgba(0, 0, 0, 0.55);
}
.tag.on {
  border-color: rgba(22, 163, 74, 0.35);
  background: rgba(22, 163, 74, 0.08);
  color: #15803d;
}
.tag.off {
  border-color: rgba(185, 28, 28, 0.3);
  background: rgba(185, 28, 28, 0.06);
  color: #b91c1c;
}
</style>
