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
/**
 * 基准价：接口口径是「元 / MB」（1 MB = 2²⁰ = 1,048,576 token），
 * 页面按人话展示成「元 / 百万 token」——换算只发生在展示与输入边界，
 * 后端存储和计费公式一行不动。
 */
const baseYuanMb = ref(1)
const baseDraft = ref('')
const editingBase = ref(false)
const editingModel = ref('')
const loading = ref(true)
const error = ref('')
const { notice, flash: flashNotice, clear: clearNotice } = useNotice()
const savingModel = ref('')
const savingBase = ref(false)

/** 每行可编辑的费率，按模型名存。未配费率留空，不显示 0。 */
const draft = reactive<Record<string, string>>({})

const editable = computed(() => can('usage:write'))

const TOKENS_PER_MB = 1048576
const TOKENS_PER_MILLION = 1000000

/** 元 / MB → 元 / 百万 token（×1.048576）。 */
function toPerMillion(yuanPerMb: number): number {
  return (yuanPerMb * TOKENS_PER_MB) / TOKENS_PER_MILLION
}

/** 元 / 百万 token → 元 / MB。 */
function fromPerMillion(yuanPerMillion: number): number {
  return (yuanPerMillion * TOKENS_PER_MILLION) / TOKENS_PER_MB
}

/**
 * 金额格式化：最多 6 位小数、去掉尾随零。
 *
 * 必须保精度——如果显示成 1.05 再原样保存回去，价格会被悄悄改掉 0.14%。
 */
function fmtMoney(v: number): string {
  return String(Number(v.toFixed(6)))
}

const basePerMillion = computed(() => toPerMillion(baseYuanMb.value))
const baseDisplay = computed(() => fmtMoney(basePerMillion.value))

/** 某费率对应的单价（元 / 百万 token）。 */
function unitPrice(ratio: number): string {
  return fmtMoney(basePerMillion.value * ratio)
}

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
    baseYuanMb.value = Number(res.baseYuanPerMb ?? 1)
    syncDraft(rows.value)
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
}

function startEditBase() {
  // 预填完整精度，避免「打开编辑→直接保存」把价格改掉。
  baseDraft.value = baseDisplay.value
  editingBase.value = true
}

function cancelEditBase() {
  editingBase.value = false
  baseDraft.value = ''
}

async function saveBase() {
  const parsed = parseRatioInput(baseDraft.value)
  if (!parsed.ok) {
    error.value = t('billing.baseInvalid')
    return
  }
  error.value = ''
  savingBase.value = true
  try {
    // 页面单位是元/百万 token，接口单位是元/MB，只在这一处换算。
    await api.put('/api/admin/model-prices/base', {
      baseYuanPerMb: fromPerMillion(parsed.value),
    })
    flashNotice(t('billing.baseSaved'))
    editingBase.value = false
    await load()
  } catch (e) {
    error.value = errText(e)
  } finally {
    savingBase.value = false
  }
}

function startEditRow(row: RatioRow) {
  draft[row.model] = row.configured ? String(row.ratio) : ''
  editingModel.value = row.model
}

function cancelEditRow() {
  editingModel.value = ''
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
    editingModel.value = ''
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

      <!-- 展示态：只读，不摆输入框 -->
      <div v-if="!editingBase" class="row" style="margin-top: 12px; align-items: baseline">
        <span class="mono base-value">{{ baseDisplay }}</span>
        <span class="muted small">{{ t('billing.baseUnitPerMillion') }}</span>
        <button v-if="editable" class="tiny" @click="startEditBase">
          {{ t('common.edit') }}
        </button>
      </div>

      <!-- 编辑态：改完才回到展示态 -->
      <div v-else class="row" style="margin-top: 12px">
        <input
          v-model="baseDraft"
          class="num base"
          type="number"
          min="0"
          step="0.01"
          aria-label="基准价"
        />
        <span class="muted small">{{ t('billing.baseUnitPerMillion') }}</span>
        <button class="tiny" :disabled="savingBase" @click="saveBase">
          {{ t('billing.save') }}
        </button>
        <button class="tiny" :disabled="savingBase" @click="cancelEditBase">
          {{ t('common.cancel') }}
        </button>
      </div>
      <p v-if="editingBase" class="muted small" style="margin-top: 8px">
        {{ t('billing.baseImpact') }}
      </p>
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
            <th class="num">{{ t('billing.unitPrice') }}</th>
            <th>{{ t('billing.status') }}</th>
            <th class="actions"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.model">
            <td class="mono">{{ row.model }}</td>
            <td class="num">
              <input
                v-if="editingModel === row.model"
                v-model="draft[row.model]"
                class="num ratio"
                type="number"
                min="0"
                step="0.0001"
                :placeholder="t('billing.unpricedPlaceholder')"
                :aria-label="`${row.model} ${t('billing.ratio')}`"
              />
              <span v-else class="mono">{{ row.configured ? row.ratio : '—' }}</span>
            </td>
            <td class="num mono">{{ row.configured ? unitPrice(row.ratio) : '—' }}</td>
            <td>
              <span v-if="row.configured" class="tag on">{{ t('billing.configured') }}</span>
              <span v-else class="tag off">{{ t('billing.unpriced') }}</span>
            </td>
            <td class="actions">
              <template v-if="editable && editingModel === row.model">
                <button class="tiny" :disabled="savingModel === row.model" @click="saveRow(row)">
                  {{ t('billing.save') }}
                </button>
                <button class="tiny" :disabled="savingModel === row.model" @click="cancelEditRow">
                  {{ t('common.cancel') }}
                </button>
              </template>
              <template v-else-if="editable">
                <button class="tiny" @click="startEditRow(row)">{{ t('common.edit') }}</button>
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
            <td colspan="5" class="muted small">{{ t('billing.emptyCatalog') }}</td>
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
  width: 140px;
  text-align: right;
}
.base-value {
  font-size: 20px;
  font-weight: 600;
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
